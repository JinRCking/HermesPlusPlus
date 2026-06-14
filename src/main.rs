mod api;
mod proxy;
mod settings;

use std::sync::{Arc, Mutex};

const PROXY_PORT: u16 = 57421;
const MGMT_PORT: u16 = 57422;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let settings = {
        let path = settings::settings_path();
        let mut store = settings::SettingsStore::new(path);
        if let Err(e) = store.load() {
            eprintln!("[警告] 读取设置失败: {}", e);
        }
        store
    };

    let settings = Arc::new(Mutex::new(settings));

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    let proxy_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>> =
        Arc::new(Mutex::new(None));

    let proxy_state = proxy::ProxyState {
        settings: Arc::clone(&settings),
        client: client.clone(),
    };

    let api_state = api::ApiState {
        settings: Arc::clone(&settings),
        client,
        proxy_handle: Arc::clone(&proxy_handle),
        proxy_state: proxy_state.clone(),
    };

    tokio::spawn(api::start_mgmt_server(api_state, MGMT_PORT));

    // Give server a moment to start
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let url = format!("http://127.0.0.1:{}", MGMT_PORT);
    eprintln!("=================================");
    eprintln!("  HermesPlusPlus 已启动！");
    eprintln!("  管理界面: {}", url);
    eprintln!("  代理端口: {} (在界面中启动)", PROXY_PORT);
    eprintln!("  按 Ctrl+C 退出");
    eprintln!("=================================");

    if let Err(e) = open::that(&url) {
        eprintln!("[提示] 请手动在浏览器中打开: {} ({})", url, e);
    }

    tokio::signal::ctrl_c().await?;
    eprintln!("正在关闭...");

    Ok(())
}
