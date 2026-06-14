use std::sync::Arc;
use std::sync::Mutex;
use serde_json::Value;
use tauri::State;
use hermes_plus_core::settings::{ProviderProfile, ProviderProtocol, SettingsStore};
use hermes_plus_core::model_catalog;
use hermes_plus_core::proxy_server::{self, ProxyState};
use hermes_plus_core::relay_config;
use log::info;

pub struct AppState {
    pub settings: Arc<Mutex<SettingsStore>>,
    pub proxy_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    pub upstream_client: reqwest::Client,
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Result<Value, String> {
    let settings = state.settings.lock().map_err(|e| e.to_string())?;
    let data = settings.data();
    serde_json::to_value(data).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_active_provider(id: String, state: State<AppState>) -> Result<(), String> {
    let mut settings = state.settings.lock().map_err(|e| e.to_string())?;
    settings.set_active_provider(&id).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn update_provider(profile: Value, state: State<AppState>) -> Result<(), String> {
    let profile: ProviderProfile = serde_json::from_value(profile).map_err(|e| e.to_string())?;
    let mut settings = state.settings.lock().map_err(|e| e.to_string())?;
    settings.update_provider(profile).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn delete_provider(id: String, state: State<AppState>) -> Result<(), String> {
    let mut settings = state.settings.lock().map_err(|e| e.to_string())?;
    settings.delete_provider(&id).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_model_catalog(state: State<AppState>) -> Result<Value, String> {
    let settings = state.settings.lock().map_err(|e| e.to_string())?;
    Ok(settings.model_catalog_json())
}

#[tauri::command]
pub async fn fetch_upstream_models(
    profile_id: String,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let profile = {
        let settings = state.settings.lock().map_err(|e| e.to_string())?;
        settings.data().provider_profiles.iter()
            .find(|p| p.id == profile_id)
            .cloned()
            .unwrap_or_default()
    };

    let (models, source_status) = model_catalog::fetch_models_from_provider(
        &state.upstream_client,
        &profile,
    ).await.map_err(|e| e.to_string())?;

    Ok(serde_json::json!({
        "models": models,
        "sourceStatus": source_status,
    }))
}

#[tauri::command]
pub async fn test_provider(
    profile_id: String,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let profile = {
        let settings = state.settings.lock().map_err(|e| e.to_string())?;
        settings.data().provider_profiles.iter()
            .find(|p| p.id == profile_id)
            .cloned()
            .unwrap_or_default()
    };

    let result = relay_config::test_provider_connectivity(
        &state.upstream_client,
        &profile,
    ).await.map_err(|e| e.to_string())?;

    serde_json::to_value(result).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn apply_provider_config(
    profile_id: String,
    state: State<AppState>,
) -> Result<Value, String> {
    let profile = {
        let settings = state.settings.lock().map_err(|e| e.to_string())?;
        settings.data().provider_profiles.iter()
            .find(|p| p.id == profile_id)
            .cloned()
            .unwrap_or_default()
    };

    let home = hermes_plus_core::settings::hermes_config_dir();
    let result = relay_config::apply_provider_config(&home, &profile)
        .map_err(|e| e.to_string())?;

    serde_json::to_value(result).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn start_proxy(state: State<AppState>) -> Result<Value, String> {
    let port = {
        let settings = state.settings.lock().map_err(|e| e.to_string())?;
        settings.data().proxy_port
    };

    let proxy_state = ProxyState {
        settings: Arc::clone(&state.settings),
        upstream_client: state.upstream_client.clone(),
    };

    let handle = tokio::spawn(async move {
        if let Err(e) = proxy_server::start_proxy_server(proxy_state, port).await {
            log::error!("Failed to start proxy: {}", e);
        }
    });

    let mut proxy_handle = state.proxy_handle.lock().map_err(|e| e.to_string())?;
    *proxy_handle = Some(handle);

    info!("Proxy server started on port {}", port);

    Ok(serde_json::json!({
        "status": "ok",
        "port": port,
        "baseUrl": format!("http://127.0.0.1:{}/v1", port),
    }))
}

#[tauri::command]
pub fn stop_proxy(state: State<AppState>) -> Result<(), String> {
    let mut proxy_handle = state.proxy_handle.lock().map_err(|e| e.to_string())?;
    if let Some(handle) = proxy_handle.take() {
        handle.abort();
        info!("Proxy server stopped");
    }
    Ok(())
}

#[tauri::command]
pub fn get_proxy_status(state: State<AppState>) -> Result<Value, String> {
    let proxy_handle = state.proxy_handle.lock().map_err(|e| e.to_string())?;
    let port = {
        let settings = state.settings.lock().map_err(|e| e.to_string())?;
        settings.data().proxy_port
    };
    let running = proxy_handle.is_some();

    Ok(serde_json::json!({
        "running": running,
        "port": port,
        "baseUrl": format!("http://127.0.0.1:{}/v1", port),
    }))
}

#[tauri::command]
pub fn get_provider_status(state: State<AppState>) -> Result<Value, String> {
    let home = hermes_plus_core::settings::hermes_config_dir();
    let status = relay_config::read_provider_status(&home);
    serde_json::to_value(status).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_settings(settings: Value, state: State<AppState>) -> Result<(), String> {
    let mut store = state.settings.lock().map_err(|e| e.to_string())?;
    let data: hermes_plus_core::settings::HermesSettings = 
        serde_json::from_value(settings).map_err(|e| e.to_string())?;
    *store.data_mut() = data;
    store.save().map_err(|e| e.to_string())?;
    Ok(())
}
