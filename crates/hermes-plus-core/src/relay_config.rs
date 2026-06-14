use std::path::Path;
use serde_json::{Value, json};
use crate::settings::ProviderProfile;

pub const HERMES_PROVIDER_TABLE: &str = "hermes_plus";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    pub configured: bool,
    pub has_api_key: bool,
    pub config_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub config_path: String,
    pub backup_path: Option<String>,
    pub configured: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTestResult {
    pub http_status: u16,
    pub endpoint: String,
    pub response_preview: String,
}

pub fn apply_provider_config(
    home: &Path,
    profile: &ProviderProfile,
) -> anyhow::Result<ApplyResult> {
    let config_path = home.join("config.toml");
    std::fs::create_dir_all(home)?;

    let backup = if config_path.exists() {
        let backup = home.join("config.toml.bak");
        std::fs::copy(&config_path, &backup)?;
        Some(backup.to_string_lossy().to_string())
    } else {
        None
    };

    let mut contents = std::fs::read_to_string(&config_path).unwrap_or_default();
    contents = upsert_provider_config(&contents, profile)?;

    std::fs::write(&config_path, contents)?;

    Ok(ApplyResult {
        config_path: config_path.to_string_lossy().to_string(),
        backup_path: backup,
        configured: true,
    })
}

fn upsert_provider_config(
    contents: &str,
    profile: &ProviderProfile,
) -> anyhow::Result<String> {
    let mut doc = contents.parse::<toml_edit::DocumentMut>()
        .unwrap_or_default();

    let provider_id = HERMES_PROVIDER_TABLE;
    if !doc.as_table().contains_key(provider_id) {
        doc[provider_id] = toml_edit::table();
    }

    let provider = doc[provider_id]
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("provider must be TOML table"))?;

    provider["name"] = toml_edit::value(profile.name.as_str());
    provider["wire_api"] = toml_edit::value("chat_completions");
    provider["requires_openai_auth"] = toml_edit::value(true);
    provider["base_url"] = toml_edit::value(profile.base_url.as_str());
    provider["experimental_bearer_token"] = toml_edit::value(profile.api_key.as_str());

    if !profile.model.is_empty() {
        provider["default_model"] = toml_edit::value(profile.model.as_str());
    }

    let mut result = doc.to_string();
    if !result.ends_with('\n') {
        result.push('\n');
    }
    Ok(result)
}

pub fn read_provider_status(home: &Path) -> ProviderStatus {
    let config_path = home.join("config.toml");
    let configured = config_path.exists();
    let has_api_key = if configured {
        std::fs::read_to_string(&config_path)
            .map(|c| c.contains("experimental_bearer_token"))
            .unwrap_or(false)
    } else {
        false
    };

    ProviderStatus {
        configured,
        has_api_key,
        config_path: config_path.to_string_lossy().to_string(),
    }
}

pub async fn test_provider_connectivity(
    client: &reqwest::Client,
    profile: &ProviderProfile,
) -> anyhow::Result<ProviderTestResult> {
    let base = profile.base_url.trim().trim_end_matches('/');
    let endpoint = format!("{}/models", base);

    let request = client
        .get(&endpoint)
        .header("Authorization", format!("Bearer {}", profile.api_key))
        .header("User-Agent", &profile.user_agent);

    let response = request.send().await?;
    let status = response.status().as_u16();
    let body = response.text().await.unwrap_or_default();
    let preview = if body.len() > 256 { &body[..256] } else { &body }.to_string();

    Ok(ProviderTestResult {
        http_status: status,
        endpoint,
        response_preview: preview,
    })
}

pub fn build_model_catalog_from_upstream(
    upstream_response: &str,
) -> anyhow::Result<Vec<String>> {
    let value: Value = serde_json::from_str(upstream_response)?;
    let models = value
        .get("data")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.get("id").and_then(Value::as_str).map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(models)
}
