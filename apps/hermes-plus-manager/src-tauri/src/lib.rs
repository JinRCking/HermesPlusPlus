use std::sync::Arc;
use std::sync::Mutex;
use log::info;

mod commands;

pub use commands::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    let mut settings = hermes_plus_core::settings::default_settings();
    let _ = settings.load();

    let upstream_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .expect("Failed to build HTTP client");

    let state = AppState {
        settings: Arc::new(Mutex::new(settings)),
        proxy_handle: Arc::new(Mutex::new(None)),
        upstream_client,
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::set_active_provider,
            commands::update_provider,
            commands::delete_provider,
            commands::get_model_catalog,
            commands::fetch_upstream_models,
            commands::test_provider,
            commands::apply_provider_config,
            commands::start_proxy,
            commands::stop_proxy,
            commands::get_proxy_status,
            commands::get_provider_status,
            commands::save_settings,
        ])
        .setup(|_app| {
            info!("HermesPlusPlus manager started");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
