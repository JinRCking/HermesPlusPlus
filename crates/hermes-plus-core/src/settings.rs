use std::collections::HashMap;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use anyhow::Context;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProfile {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing)]
    pub base_url: String,
    #[serde(default, skip_serializing)]
    pub api_key: String,
    #[serde(default)]
    pub protocol: ProviderProtocol,
    #[serde(default)]
    pub model: String,
    #[serde(rename = "modelList", default)]
    pub model_list: String,
    #[serde(rename = "userAgent", default)]
    pub user_agent: String,
    #[serde(rename = "contextWindow", default)]
    pub context_window: String,
    #[serde(rename = "autoCompactLimit", default)]
    pub auto_compact_limit: String,
}

impl Default for ProviderProfile {
    fn default() -> Self {
        Self {
            id: "default".to_string(),
            name: "默认供应商".to_string(),
            base_url: String::new(),
            api_key: String::new(),
            protocol: ProviderProtocol::ChatCompletions,
            model: String::new(),
            model_list: String::new(),
            user_agent: String::new(),
            context_window: String::new(),
            auto_compact_limit: String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ProviderProtocol {
    #[default]
    ChatCompletions,
    Responses,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HermesSettings {
    #[serde(rename = "hermesAppPath", default)]
    pub hermes_app_path: String,
    #[serde(rename = "hermesExtraArgs", default)]
    pub hermes_extra_args: Vec<String>,
    #[serde(rename = "providerProfilesEnabled", default = "default_true")]
    pub provider_profiles_enabled: bool,
    #[serde(rename = "providerProfiles", default = "default_provider_profiles")]
    pub provider_profiles: Vec<ProviderProfile>,
    #[serde(rename = "activeProviderId", default = "default_active_provider_id")]
    pub active_provider_id: String,
    #[serde(rename = "proxyPort", default = "default_proxy_port")]
    pub proxy_port: u16,
    #[serde(rename = "testModel", default)]
    pub test_model: String,
    #[serde(rename = "enhancementsEnabled", default = "default_true")]
    pub enhancements_enabled: bool,
    #[serde(rename = "modelWhitelistUnlock", default = "default_true")]
    pub model_whitelist_unlock: bool,
    #[serde(rename = "sessionDelete", default = "default_true")]
    pub session_delete: bool,
    #[serde(rename = "markdownExport", default = "default_true")]
    pub markdown_export: bool,
    #[serde(rename = "nativeMenuPlacement", default = "default_true")]
    pub native_menu_placement: bool,
    #[serde(rename = "serviceTierControls", default)]
    pub service_tier_controls: bool,
}

impl Default for HermesSettings {
    fn default() -> Self {
        Self {
            hermes_app_path: String::new(),
            hermes_extra_args: Vec::new(),
            provider_profiles_enabled: true,
            provider_profiles: vec![ProviderProfile::default()],
            active_provider_id: "default".to_string(),
            proxy_port: 57421,
            test_model: String::new(),
            enhancements_enabled: true,
            model_whitelist_unlock: true,
            session_delete: true,
            markdown_export: true,
            native_menu_placement: true,
            service_tier_controls: false,
        }
    }
}

fn default_true() -> bool { true }
fn default_provider_profiles() -> Vec<ProviderProfile> { vec![ProviderProfile::default()] }
fn default_active_provider_id() -> String { "default".to_string() }
fn default_proxy_port() -> u16 { 57421 }

pub fn hermes_config_dir() -> PathBuf {
    std::env::var_os("HERMES_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            directories::BaseDirs::new()
                .map(|dirs| dirs.home_dir().join(".hermes-plus"))
                .unwrap_or_else(|| PathBuf::from(".hermes-plus"))
        })
}

pub fn settings_path() -> PathBuf {
    hermes_config_dir().join("settings.json")
}

pub fn settings_backup_path() -> PathBuf {
    hermes_config_dir().join("settings.json.bak")
}

#[derive(Debug, Clone, Default)]
pub struct SettingsStore {
    path: PathBuf,
    backup: PathBuf,
    data: HermesSettings,
}

impl SettingsStore {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            backup: settings_backup_path(),
            data: HermesSettings::default(),
        }
    }

    pub fn load(&mut self) -> anyhow::Result<&HermesSettings> {
        if self.path.exists() {
            let content = std::fs::read_to_string(&self.path)?;
            self.data = serde_json::from_str(&content)
                .context("解析 settings.json 失败")?;
        }
        Ok(&self.data)
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let content = serde_json::to_string_pretty(&self.data)?;
        if self.path.exists() {
            std::fs::copy(&self.path, &self.backup)?;
        }
        std::fs::create_dir_all(self.path.parent().unwrap_or(Path::new(".")))?;
        std::fs::write(&self.path, content)?;
        Ok(())
    }

    pub fn data(&self) -> &HermesSettings { &self.data }
    pub fn data_mut(&mut self) -> &mut HermesSettings { &mut self.data }

    pub fn active_provider(&self) -> ProviderProfile {
        let active_id = self.data.active_provider_id.clone();
        self.data.provider_profiles
            .iter()
            .find(|p| p.id == active_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn set_active_provider(&mut self, id: &str) -> anyhow::Result<()> {
        if self.data.provider_profiles.iter().any(|p| p.id == id) {
            self.data.active_provider_id = id.to_string();
            self.save()?;
        }
        Ok(())
    }

    pub fn update_provider(&mut self, profile: ProviderProfile) -> anyhow::Result<()> {
        let pos = self.data.provider_profiles.iter()
            .position(|p| p.id == profile.id);
        if let Some(pos) = pos {
            self.data.provider_profiles[pos] = profile;
        } else {
            self.data.provider_profiles.push(profile);
        }
        self.save()?;
        Ok(())
    }

    pub fn delete_provider(&mut self, id: &str) -> anyhow::Result<()> {
        self.data.provider_profiles.retain(|p| p.id != id);
        if self.data.active_provider_id == id {
            self.data.active_provider_id = self.data.provider_profiles
                .first()
                .map(|p| p.id.clone())
                .unwrap_or_default();
        }
        self.save()?;
        Ok(())
    }

    pub fn model_catalog_json(&self) -> Value {
        let active = self.active_provider();
        let models = active.model_list
            .split(['\r', '\n', ','])
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        let default_model = if models.contains(&active.model) {
            active.model.clone()
        } else {
            models.first().cloned().unwrap_or_default()
        };
        serde_json::json!({
            "status": if models.is_empty() { "not_configured" } else { "ok" },
            "providerId": active.id,
            "providerName": active.name,
            "model": active.model,
            "defaultModel": default_model,
            "models": models,
            "baseUrl": active.base_url,
            "protocol": active.protocol,
            "sources": [{
                "id": format!("provider:{}:model_list", active.id),
                "type": "provider_model_list",
                "name": active.name,
                "baseUrl": active.base_url,
                "status": if models.is_empty() { "not_configured" } else { "ok" },
                "models": models.len(),
            }]
        })
    }
}

pub fn default_settings() -> SettingsStore {
    SettingsStore::new(settings_path())
}
