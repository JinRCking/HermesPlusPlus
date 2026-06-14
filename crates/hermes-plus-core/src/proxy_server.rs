use std::net::SocketAddr;
use std::sync::Arc;
use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{Request, Response, StatusCode},
    response::IntoResponse,
    routing::any,
    Router,
};
use serde_json::{Value, json};
use tower_http::cors::{Any, CorsLayer};
use log::{info, error, debug};
use crate::settings::{ProviderProfile, ProviderProtocol, SettingsStore};

pub const DEFAULT_PROXY_PORT: u16 = 57421;

#[derive(Clone)]
pub struct ProxyState {
    pub settings: Arc<std::sync::Mutex<SettingsStore>>,
    pub upstream_client: reqwest::Client,
}

pub fn local_proxy_base_url(port: u16) -> String {
    format!("http://127.0.0.1:{}/v1", port)
}

pub async fn start_proxy_server(
    state: ProxyState,
    port: u16,
) -> anyhow::Result<tokio::task::JoinHandle<()>> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/v1/{*path}", any(handle_request))
        .route("/v1/models", axum::routing::get(handle_models))
        .route("/health", axum::routing::get(handle_health))
        .layer(cors)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("HermesPlusPlus proxy server listening on {}", addr);

    let handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            error!("Proxy server error: {}", e);
        }
    });

    Ok(handle)
}

async fn handle_health() -> impl IntoResponse {
    JsonResponse(json!({ "status": "ok", "app": "HermesPlusPlus" }))
}

async fn handle_models(State(state): State<ProxyState>) -> impl IntoResponse {
    let settings = state.settings.lock().unwrap();
    let profile = settings.active_provider();
    let models = crate::model_catalog::profile_model_ids(&profile);

    let data: Vec<Value> = models.into_iter()
        .map(|id| json!({ "id": id, "object": "model", "owned_by": profile.name }))
        .collect();

    JsonResponse(json!({ "object": "list", "data": data }))
}

async fn handle_request(
    State(state): State<ProxyState>,
    req: Request<Body>,
) -> impl IntoResponse {
    let path = req.uri().path().trim_start_matches("/v1/");
    let method = req.method().clone();
    debug!("Proxy request: {} {}", method, path);

    let settings = state.settings.lock().unwrap();
    let profile = settings.active_provider();

    if profile.base_url.is_empty() {
        return error_response(StatusCode::BAD_GATEWAY, "upstream not configured");
    }

    let upstream_url = format!("{}/{}", profile.base_url.trim_end_matches('/'), path);
    let upstream_url = if let Some(query) = req.uri().query() {
        format!("{}?{}", upstream_url, query)
    } else {
        upstream_url
    };

    let mut request_builder = state.upstream_client
        .request(method, &upstream_url)
        .header("Authorization", format!("Bearer {}", profile.api_key));

    if !profile.user_agent.is_empty() {
        request_builder = request_builder.header("User-Agent", &profile.user_agent);
    }

    let body_bytes = match axum::body::to_bytes(req.into_body(), usize::MAX).await {
        Ok(bytes) => bytes,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, &e.to_string()),
    };

    let body_bytes = if path.contains("/responses") && profile.protocol == ProviderProtocol::ChatCompletions {
        match transform_responses_to_chat(&body_bytes) {
            Ok(transformed) => transformed,
            Err(e) => {
                error!("Transform error: {}", e);
                body_bytes
            }
        }
    } else {
        body_bytes
    };

    let request_builder = request_builder.body(body_bytes);

    match request_builder.send().await {
        Ok(resp) => {
            let status = StatusCode::from_u16(resp.status().as_u16())
                .unwrap_or(StatusCode::OK);
            let mut response_builder = Response::builder().status(status);

            for (key, value) in resp.headers() {
                if key != "transfer-encoding" && key != "content-encoding" {
                    response_builder = response_builder.header(key, value);
                }
            }

            let body_bytes = match resp.bytes().await {
                Ok(bytes) => bytes,
                Err(e) => return error_response(StatusCode::BAD_GATEWAY, &e.to_string()),
            };

            let body_bytes = if path.contains("/responses") && profile.protocol == ProviderProtocol::ChatCompletions {
                match transform_chat_to_responses(&body_bytes) {
                    Ok(transformed) => transformed,
                    Err(e) => {
                        error!("Response transform error: {}", e);
                        body_bytes
                    }
                }
            } else {
                body_bytes
            };

            match response_builder.body(Body::from(body_bytes)) {
                Ok(response) => response,
                Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
            }
        }
        Err(e) => {
            error!("Upstream request failed: {}", e);
            error_response(StatusCode::BAD_GATEWAY, &e.to_string())
        }
    }
}

fn error_response(status: StatusCode, message: &str) -> Response<Body> {
    let body = json!({ "error": { "message": message, "type": "proxy_error" } });
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn transform_responses_to_chat(body: &Bytes) -> anyhow::Result<Bytes> {
    let value: Value = serde_json::from_slice(body)?;
    let mut result = json!({});

    if let Some(model) = value.get("model") {
        result["model"] = model.clone();
    }

    let mut messages = Vec::new();
    if let Some(instructions) = value.get("instructions") {
        let text = instruction_text(instructions);
        if !text.is_empty() {
            messages.push(json!({ "role": "system", "content": text }));
        }
    }

    if let Some(input) = value.get("input") {
        append_responses_input(input, &mut messages);
    }

    normalize_chat_messages(&mut messages);
    let messages = collapse_system_messages_to_head(messages);
    result["messages"] = json!(messages);

    for key in &["temperature", "top_p", "max_tokens", "stream", "tools", "tool_choice"] {
        if let Some(v) = value.get(key) {
            result[*key] = v.clone();
        }
    }

    if let Some(max_output) = value.get("max_output_tokens") {
        result["max_tokens"] = max_output.clone();
    }

    let json_str = serde_json::to_string(&result)?;
    Ok(Bytes::from(json_str))
}

fn transform_chat_to_responses(body: &Bytes) -> anyhow::Result<Bytes> {
    let value: Value = serde_json::from_slice(body)?;
    let mut result = json!({
        "object": "response",
        "status": "completed",
        "output": [],
    });

    if let Some(id) = value.get("id") {
        result["id"] = id.clone();
    }
    if let Some(model) = value.get("model") {
        result["model"] = model.clone();
    }
    if let Some(created) = value.get("created") {
        result["created_at"] = created.clone();
    }

    if let Some(choices) = value.get("choices").and_then(Value::as_array) {
        let mut output = Vec::new();
        for choice in choices {
            if let Some(message) = choice.get("message") {
                let mut item = json!({
                    "type": "message",
                    "role": "assistant",
                    "content": [],
                });

                if let Some(content) = message.get("content").and_then(Value::as_str) {
                    item["content"] = json!([{
                        "type": "output_text",
                        "text": content,
                    }]);
                }
                output.push(item);
            }
        }
        result["output"] = json!(output);
    }

    if let Some(usage) = value.get("usage") {
        result["usage"] = json!({
            "input_tokens": usage.get("prompt_tokens").unwrap_or(&json!(0)),
            "output_tokens": usage.get("completion_tokens").unwrap_or(&json!(0)),
            "total_tokens": usage.get("total_tokens").unwrap_or(&json!(0)),
        });
    }

    let json_str = serde_json::to_string(&result)?;
    Ok(Bytes::from(json_str))
}

fn instruction_text(instructions: &Value) -> String {
    match instructions {
        Value::String(s) => s.clone(),
        Value::Array(arr) => arr.iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn append_responses_input(input: &Value, messages: &mut Vec<Value>) {
    match input {
        Value::String(text) => {
            messages.push(json!({ "role": "user", "content": text }));
        }
        Value::Array(arr) => {
            for item in arr {
                if let Some(role) = item.get("role").and_then(Value::as_str) {
                    let content = item_content_text(item);
                    messages.push(json!({ "role": role, "content": content }));
                }
            }
        }
        _ => {}
    }
}

fn item_content_text(item: &Value) -> String {
    match item.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(arr)) => arr.iter()
            .filter_map(|c| match c.get("type").and_then(Value::as_str) {
                Some("input_text") => c.get("text").and_then(Value::as_str).map(String::from),
                Some("input_image") => c.get("image_url").and_then(Value::as_str).map(String::from),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn normalize_chat_messages(messages: &mut Vec<Value>) {
    for msg in messages.iter_mut() {
        if let Some(content) = msg.get("content").and_then(Value::as_array) {
            let text_parts: Vec<String> = content.iter()
                .filter_map(|part| {
                    if part.get("type").and_then(Value::as_str) == Some("text") {
                        part.get("text").and_then(Value::as_str).map(String::from)
                    } else {
                        None
                    }
                })
                .collect();
            if !text_parts.is_empty() {
                msg["content"] = json!(text_parts.join("\n"));
            }
        }
    }
}

fn collapse_system_messages_to_head(messages: Vec<Value>) -> Vec<Value> {
    let mut system_texts: Vec<String> = Vec::new();
    let mut non_system: Vec<Value> = Vec::new();

    for msg in messages {
        if msg.get("role").and_then(Value::as_str) == Some("system") {
            if let Some(text) = msg.get("content").and_then(Value::as_str) {
                system_texts.push(text.to_string());
            }
        } else {
            non_system.push(msg);
        }
    }

    let mut result = Vec::new();
    if !system_texts.is_empty() {
        result.push(json!({ "role": "system", "content": system_texts.join("\n\n") }));
    }
    result.extend(non_system);
    result
}

struct JsonResponse(Value);

impl IntoResponse for JsonResponse {
    fn into_response(self) -> Response<Body> {
        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Body::from(self.0.to_string()))
            .unwrap()
    }
}
