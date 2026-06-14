use axum::{
    body::Body,
    extract::State,
    http::{HeaderValue, Request, Response, StatusCode},
    Router,
};
use serde_json::json;
use std::sync::{Arc, Mutex};

use crate::settings::SettingsStore;

#[derive(Clone)]
pub struct ProxyState {
    pub settings: Arc<Mutex<SettingsStore>>,
    pub client: reqwest::Client,
}

pub async fn run_proxy(state: ProxyState, listener: tokio::net::TcpListener) -> anyhow::Result<()> {
    let port = listener.local_addr()?.port();
    // Use a single catch-all fallback — avoids axum route conflicts between
    // literal /v1/models and wildcard /v1/{*path} when both handle GET.
    let app = Router::new()
        .fallback(handle_any)
        .with_state(state);

    eprintln!("[代理] 运行于 http://127.0.0.1:{}/v1", port);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn handle_any(State(state): State<ProxyState>, req: Request<Body>) -> Response<Body> {
    // CORS preflight
    if req.method() == axum::http::Method::OPTIONS {
        return cors_ok();
    }

    let path = req.uri().path();

    // Health check
    if path == "/health" {
        return cors_json(StatusCode::OK, json!({"status": "ok"}).to_string());
    }

    // Serve local model list without forwarding upstream
    if path == "/v1/models" && req.method() == axum::http::Method::GET {
        return serve_models(&state);
    }

    proxy_upstream(state, req).await
}

fn serve_models(state: &ProxyState) -> Response<Body> {
    let models = {
        let settings = state.settings.lock().unwrap();
        settings.model_ids()
    };
    let data: Vec<_> = models
        .iter()
        .map(|id| json!({ "id": id, "object": "model" }))
        .collect();
    cors_json(
        StatusCode::OK,
        json!({ "object": "list", "data": data }).to_string(),
    )
}

async fn proxy_upstream(state: ProxyState, req: Request<Body>) -> Response<Body> {
    let profile = {
        let settings = state.settings.lock().unwrap();
        match settings.active_provider() {
            Some(p) => p,
            None => return err_resp(StatusCode::BAD_GATEWAY, "没有配置活跃供应商"),
        }
    };

    if profile.base_url.is_empty() {
        return err_resp(StatusCode::BAD_GATEWAY, "供应商 base_url 未设置");
    }

    // Strip /v1 prefix and rebuild upstream URL
    let path_str = req.uri().path();
    let stripped = path_str.strip_prefix("/v1").unwrap_or(path_str);
    let query_part = req
        .uri()
        .query()
        .map(|q| format!("?{}", q))
        .unwrap_or_default();
    let upstream_url = format!(
        "{}{}{}",
        profile.base_url.trim_end_matches('/'),
        stripped,
        query_part
    );

    let method = reqwest::Method::from_bytes(req.method().as_str().as_bytes())
        .unwrap_or(reqwest::Method::POST);
    let mut builder = state.client.request(method, &upstream_url);

    for (k, v) in req.headers() {
        let kl = k.as_str();
        if matches!(
            kl,
            "host" | "connection" | "transfer-encoding" | "authorization" | "x-api-key"
                | "accept-encoding"
        ) {
            continue;
        }
        if let Ok(hv) = reqwest::header::HeaderValue::from_bytes(v.as_bytes()) {
            builder = builder.header(kl, hv);
        }
    }

    builder = builder.header("authorization", format!("Bearer {}", profile.api_key));
    builder = builder.header("x-api-key", &profile.api_key);
    builder = builder.header("accept-encoding", "identity");

    if !profile.user_agent.is_empty() {
        builder = builder.header("user-agent", &profile.user_agent);
    }

    let body_bytes = match axum::body::to_bytes(req.into_body(), 64 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => return err_resp(StatusCode::BAD_REQUEST, &e.to_string()),
    };
    builder = builder.body(reqwest::Body::from(body_bytes.to_vec()));

    match builder.send().await {
        Ok(resp) => {
            let status =
                StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::OK);
            let mut rb = Response::builder().status(status);

            for (k, v) in resp.headers() {
                let kl = k.as_str();
                if matches!(kl, "transfer-encoding" | "content-encoding" | "content-length") {
                    continue;
                }
                rb = rb.header(kl, v.as_bytes());
            }

            let body = resp.bytes().await.unwrap_or_default();
            add_cors(rb.body(Body::from(body)).unwrap_or_else(|_| {
                err_resp(StatusCode::INTERNAL_SERVER_ERROR, "build failed")
            }))
        }
        Err(e) => err_resp(StatusCode::BAD_GATEWAY, &e.to_string()),
    }
}

fn cors_ok() -> Response<Body> {
    add_cors(
        Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(Body::empty())
            .unwrap(),
    )
}

fn cors_json(status: StatusCode, body: String) -> Response<Body> {
    add_cors(
        Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap(),
    )
}

fn add_cors(mut r: Response<Body>) -> Response<Body> {
    let h = r.headers_mut();
    h.insert(
        "access-control-allow-origin",
        HeaderValue::from_static("*"),
    );
    h.insert(
        "access-control-allow-methods",
        HeaderValue::from_static("GET, POST, PUT, DELETE, PATCH, OPTIONS"),
    );
    h.insert(
        "access-control-allow-headers",
        HeaderValue::from_static(
            "Content-Type, Authorization, x-api-key, anthropic-version, anthropic-beta",
        ),
    );
    r
}

fn err_resp(status: StatusCode, msg: &str) -> Response<Body> {
    add_cors(
        Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"error": {"message": msg, "type": "proxy_error"}}).to_string(),
            ))
            .unwrap(),
    )
}
