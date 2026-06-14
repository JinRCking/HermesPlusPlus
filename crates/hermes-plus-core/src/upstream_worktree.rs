use std::path::{Path, PathBuf};
use serde_json::{Value, json};
use crate::settings::{HermesSettings, SettingsStore};

pub const HERMES_CONFIG_DIR: &str = ".hermes-plus";

pub fn default_hermes_home_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().join(HERMES_CONFIG_DIR))
        .unwrap_or_else(|| PathBuf::from(HERMES_CONFIG_DIR))
}

pub fn settings_json_path() -> PathBuf {
    default_hermes_home_dir().join("settings.json")
}

pub fn config_toml_path() -> PathBuf {
    default_hermes_home_dir().join("config.toml")
}

pub fn auth_json_path() -> PathBuf {
    default_hermes_home_dir().join("auth.json")
}

pub fn ensure_hermes_dirs() -> anyhow::Result<()> {
    let home = default_hermes_home_dir();
    std::fs::create_dir_all(&home)?;
    Ok(())
}

pub fn read_auth_json() -> Option<Value> {
    let path = auth_json_path();
    if !path.exists() {
        return None;
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

pub fn write_auth_json(value: &Value) -> anyhow::Result<()> {
    let path = auth_json_path();
    ensure_hermes_dirs()?;
    let content = serde_json::to_string_pretty(value)?;
    std::fs::write(&path, content)?;
    Ok(())
}

pub fn read_settings_json() -> Option<HermesSettings> {
    let path = settings_json_path();
    if !path.exists() {
        return None;
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

pub fn write_settings_json(settings: &HermesSettings) -> anyhow::Result<()> {
    let path = settings_json_path();
    ensure_hermes_dirs()?;
    let content = serde_json::to_string_pretty(settings)?;
    std::fs::write(&path, content)?;
    Ok(())
}
