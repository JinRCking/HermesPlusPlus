use axum::{
    body::Body,
    extract::{Path, State},
    http::{Response, StatusCode},
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::task::JoinHandle;

use crate::proxy::ProxyState;
use crate::settings::{ProviderProfile, SettingsStore};

const UI_HTML: &str = include_str!("ui.html");

#[derive(Clone)]
pub struct ApiState {
    pub settings: Arc<Mutex<SettingsStore>>,
    pub client: reqwest::Client,
    pub proxy_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
    pub proxy_state: ProxyState,
}

pub async fn start_mgmt_server(state: ApiState, port: u16) {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let app = Router::new()
        .route("/", get(serve_ui))
        .route("/api/settings", get(h_get_settings).put(h_save_settings))
        .route("/api/providers", post(h_upsert_provider))
        .route("/api/providers/:id", delete(h_delete_provider))
        .route("/api/providers/:id/activate", post(h_activate_provider))
        .route("/api/providers/:id/test", get(h_test_provider))
        .route("/api/providers/:id/fetch-models", post(h_fetch_models))
        .route("/api/proxy/status", get(h_proxy_status))
        .route("/api/proxy/start", post(h_start_proxy))
        .route("/api/proxy/stop", post(h_stop_proxy))
        .route("/api/hermes/check", get(h_hermes_check))
        .route("/api/hermes/launch", post(h_hermes_launch))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind management port");
    eprintln!("[管理界面] http://127.0.0.1:{}", port);
    axum::serve(listener, app).await.ok();
}

async fn serve_ui() -> impl IntoResponse {
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/html; charset=utf-8")
        .body(Body::from(UI_HTML))
        .unwrap()
}

async fn h_get_settings(State(s): State<ApiState>) -> impl IntoResponse {
    let settings = s.settings.lock().unwrap();
    Json(json!(&settings.data))
}

async fn h_save_settings(
    State(s): State<ApiState>,
    Json(data): Json<Value>,
) -> impl IntoResponse {
    let mut settings = s.settings.lock().unwrap();
    match serde_json::from_value(data) {
        Ok(new_data) => {
            settings.data = new_data;
            match settings.save() {
                Ok(_) => Json(json!({"ok": true})),
                Err(e) => Json(json!({"error": e.to_string()})),
            }
        }
        Err(e) => Json(json!({"error": e.to_string()})),
    }
}

async fn h_upsert_provider(
    State(s): State<ApiState>,
    Json(profile): Json<ProviderProfile>,
) -> impl IntoResponse {
    let mut settings = s.settings.lock().unwrap();
    settings.update_provider(profile);
    let _ = settings.save();
    Json(json!({"ok": true}))
}

async fn h_delete_provider(
    State(s): State<ApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let mut settings = s.settings.lock().unwrap();
    settings.delete_provider(&id);
    let _ = settings.save();
    Json(json!({"ok": true}))
}

async fn h_activate_provider(
    State(s): State<ApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let mut settings = s.settings.lock().unwrap();
    if settings.set_active(&id) {
        let _ = settings.save();
        Json(json!({"ok": true}))
    } else {
        Json(json!({"error": "provider not found"}))
    }
}

async fn h_test_provider(
    State(s): State<ApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let profile = {
        let settings = s.settings.lock().unwrap();
        settings
            .data
            .provider_profiles
            .iter()
            .find(|p| p.id == id)
            .cloned()
    };

    let profile = match profile {
        Some(p) => p,
        None => return Json(json!({"error": "provider not found"})),
    };

    if profile.base_url.is_empty() {
        return Json(json!({"error": "base_url not set"}));
    }

    let url = format!("{}/models", profile.base_url.trim_end_matches('/'));
    match s
        .client
        .get(&url)
        .header("Authorization", format!("Bearer {}", profile.api_key))
        .header("x-api-key", &profile.api_key)
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            let preview = body.chars().take(300).collect::<String>();
            Json(json!({
                "httpStatus": status,
                "endpoint": url,
                "responsePreview": preview,
            }))
        }
        Err(e) => Json(json!({"error": e.to_string()})),
    }
}

async fn h_fetch_models(
    State(s): State<ApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let profile = {
        let settings = s.settings.lock().unwrap();
        settings
            .data
            .provider_profiles
            .iter()
            .find(|p| p.id == id)
            .cloned()
    };

    let profile = match profile {
        Some(p) => p,
        None => return Json(json!({"error": "provider not found"})),
    };

    if profile.base_url.is_empty() {
        return Json(json!({"error": "base_url not set"}));
    }

    let url = format!("{}/models", profile.base_url.trim_end_matches('/'));
    match s
        .client
        .get(&url)
        .header("Authorization", format!("Bearer {}", profile.api_key))
        .header("x-api-key", &profile.api_key)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            let body: Value = resp.json().await.unwrap_or(json!({}));
            let models: Vec<String> = body
                .get("data")
                .and_then(|d| d.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|item| {
                            item.get("id").and_then(|v| v.as_str()).map(String::from)
                        })
                        .collect()
                })
                .unwrap_or_default();
            Json(json!({"models": models}))
        }
        Ok(resp) => Json(json!({"error": format!("HTTP {}", resp.status())})),
        Err(e) => Json(json!({"error": e.to_string()})),
    }
}

async fn h_proxy_status(State(s): State<ApiState>) -> impl IntoResponse {
    let port = {
        let settings = s.settings.lock().unwrap();
        settings.data.proxy_port
    };
    let handle = s.proxy_handle.lock().unwrap();
    let running = handle.as_ref().map(|h| !h.is_finished()).unwrap_or(false);
    Json(json!({
        "running": running,
        "port": port,
        "url": format!("http://127.0.0.1:{}/v1", port),
    }))
}

async fn h_start_proxy(State(s): State<ApiState>) -> impl IntoResponse {
    let port = {
        let settings = s.settings.lock().unwrap();
        settings.data.proxy_port
    };

    // Check if already running
    {
        let handle = s.proxy_handle.lock().unwrap();
        if handle.as_ref().map(|h| !h.is_finished()).unwrap_or(false) {
            return Json(json!({"ok": true, "message": "already running", "port": port}));
        }
    }

    // Pre-bind port here so we can return a real error if it's taken
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            return Json(json!({
                "error": format!("无法绑定端口 {}: {}", port, e)
            }));
        }
    };

    let proxy_state = s.proxy_state.clone();
    let handle = tokio::spawn(async move {
        if let Err(e) = crate::proxy::run_proxy(proxy_state, listener).await {
            eprintln!("[代理] 错误: {}", e);
        }
    });

    *s.proxy_handle.lock().unwrap() = Some(handle);
    Json(json!({"ok": true, "port": port}))
}

async fn h_stop_proxy(State(s): State<ApiState>) -> impl IntoResponse {
    let mut handle = s.proxy_handle.lock().unwrap();
    if let Some(h) = handle.take() {
        h.abort();
    }
    Json(json!({"ok": true}))
}

async fn h_hermes_check(State(s): State<ApiState>) -> impl IntoResponse {
    let hermes_path = {
        let settings = s.settings.lock().unwrap();
        settings.data.hermes_path.clone()
    };

    let configured = !hermes_path.is_empty();
    let exists = configured && std::path::Path::new(&hermes_path).exists();
    Json(json!({
        "configured": configured,
        "exists": exists,
        "path": hermes_path,
    }))
}

async fn h_hermes_launch(State(s): State<ApiState>) -> impl IntoResponse {
    let (hermes_path, port) = {
        let settings = s.settings.lock().unwrap();
        (
            settings.data.hermes_path.clone(),
            settings.data.proxy_port,
        )
    };

    if hermes_path.is_empty() {
        return Json(json!({"error": "未配置 Hermes 路径，请先在设置中填写"}));
    }

    if !std::path::Path::new(&hermes_path).exists() {
        return Json(json!({"error": format!("文件不存在: {}", hermes_path)}));
    }

    let proxy_url = format!("http://127.0.0.1:{}", port);

    match std::process::Command::new(&hermes_path)
        .env("ANTHROPIC_BASE_URL", format!("{}/v1", proxy_url))
        .env("OPENAI_BASE_URL", format!("{}/v1", proxy_url))
        .env("OPENAI_API_BASE", format!("{}/v1", proxy_url))
        .spawn()
    {
        Ok(_) => Json(json!({"ok": true})),
        Err(e) => Json(json!({"error": e.to_string()})),
    }
}
