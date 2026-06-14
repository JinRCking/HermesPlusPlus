use serde_json::{Value, json};
use crate::settings::{ProviderProfile, SettingsStore};
use crate::relay_config::build_model_catalog_from_upstream;

pub async fn read_model_catalog(settings: &SettingsStore) -> Value {
    let profile = settings.active_provider();
    let models = profile_model_ids(&profile);
    let default_model = if models.contains(&profile.model) {
        profile.model.clone()
    } else {
        models.first().cloned().unwrap_or_default()
    };

    let status = if models.is_empty() { "not_configured" } else { "ok" };

    json!({
        "status": status,
        "providerId": profile.id,
        "providerName": profile.name,
        "model": profile.model,
        "defaultModel": default_model,
        "models": models,
        "baseUrl": profile.base_url,
        "sources": [{
            "id": format!("provider:{}:model_list", profile.id),
            "type": "provider_model_list",
            "name": profile.name,
            "baseUrl": profile.base_url,
            "status": status,
            "models": models.len(),
        }]
    })
}

pub async fn fetch_models_from_provider(
    client: &reqwest::Client,
    profile: &ProviderProfile,
) -> anyhow::Result<(Vec<String>, Value)> {
    let base = profile.base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Ok((Vec::new(), json!({"status": "not_configured", "message": "base_url not set"})));
    }

    let endpoint = format!("{}/models", base);
    let request = client
        .get(&endpoint)
        .header("Authorization", format!("Bearer {}", profile.api_key))
        .header("User-Agent", &profile.user_agent);

    let response = request.send().await;
    match response {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            if status == 200 {
                match build_model_catalog_from_upstream(&body) {
                    Ok(models) => {
                        let source_status = json!({
                            "id": format!("provider:{}:upstream", profile.id),
                            "type": "upstream_fetch",
                            "name": profile.name,
                            "baseUrl": base,
                            "status": "ok",
                            "models": models.len(),
                        });
                        Ok((models, source_status))
                    }
                    Err(e) => {
                        let source_status = json!({
                            "id": format!("provider:{}:upstream", profile.id),
                            "type": "upstream_fetch",
                            "name": profile.name,
                            "baseUrl": base,
                            "status": "parse_failed",
                            "message": e.to_string(),
                        });
                        Ok((Vec::new(), source_status))
                    }
                }
            } else {
                let source_status = json!({
                    "id": format!("provider:{}:upstream", profile.id),
                    "type": "upstream_fetch",
                    "name": profile.name,
                    "baseUrl": base,
                    "status": "failed",
                    "httpStatus": status,
                    "message": body,
                });
                Ok((Vec::new(), source_status))
            }
        }
        Err(e) => {
            let source_status = json!({
                "id": format!("provider:{}:upstream", profile.id),
                "type": "upstream_fetch",
                "name": profile.name,
                "baseUrl": base,
                "status": "failed",
                "message": e.to_string(),
            });
            Ok((Vec::new(), source_status))
        }
    }
}

pub fn profile_model_ids(profile: &ProviderProfile) -> Vec<String> {
    let mut models: Vec<String> = profile.model_list
        .split(['\r', '\n', ','])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if !profile.model.is_empty() && !models.contains(&profile.model) {
        models.insert(0, profile.model.clone());
    }
    models
}

pub fn merge_model_lists(lists: Vec<Vec<String>>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for list in lists {
        for model in list {
            if seen.insert(model.clone()) {
                result.push(model);
            }
        }
    }
    result
}
