use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ProviderProtocol {
    #[default]
    ChatCompletions,
    Responses,
    AnthropicMessages,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub protocol: ProviderProtocol,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub model_list: String,
    #[serde(default)]
    pub user_agent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default)]
    pub provider_profiles: Vec<ProviderProfile>,
    #[serde(default)]
    pub active_provider_id: String,
    #[serde(default = "default_proxy_port")]
    pub proxy_port: u16,
    #[serde(default)]
    pub hermes_path: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            provider_profiles: vec![],
            active_provider_id: String::new(),
            proxy_port: 57421,
            hermes_path: String::new(),
        }
    }
}

fn default_proxy_port() -> u16 {
    57421
}

pub fn settings_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|d| d.config_dir().join("hermes-plus"))
        .unwrap_or_else(|| PathBuf::from("hermes-plus"))
}

pub fn settings_path() -> PathBuf {
    settings_dir().join("settings.json")
}

pub struct SettingsStore {
    pub path: PathBuf,
    pub data: AppSettings,
}

impl SettingsStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            data: AppSettings::default(),
        }
    }

    pub fn load(&mut self) -> anyhow::Result<()> {
        if self.path.exists() {
            let content = std::fs::read_to_string(&self.path)?;
            self.data = serde_json::from_str(&content)?;
        }
        Ok(())
    }

    pub fn save(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.path, serde_json::to_string_pretty(&self.data)?)?;
        Ok(())
    }

    pub fn active_provider(&self) -> Option<ProviderProfile> {
        self.data
            .provider_profiles
            .iter()
            .find(|p| p.id == self.data.active_provider_id)
            .cloned()
    }

    pub fn model_ids(&self) -> Vec<String> {
        self.active_provider()
            .map(|p| parse_model_list(&p))
            .unwrap_or_default()
    }

    pub fn update_provider(&mut self, profile: ProviderProfile) {
        if let Some(pos) = self
            .data
            .provider_profiles
            .iter()
            .position(|p| p.id == profile.id)
        {
            self.data.provider_profiles[pos] = profile;
        } else {
            if self.data.provider_profiles.is_empty() {
                self.data.active_provider_id = profile.id.clone();
            }
            self.data.provider_profiles.push(profile);
        }
    }

    pub fn delete_provider(&mut self, id: &str) {
        self.data.provider_profiles.retain(|p| p.id != id);
        if self.data.active_provider_id == id {
            self.data.active_provider_id = self
                .data
                .provider_profiles
                .first()
                .map(|p| p.id.clone())
                .unwrap_or_default();
        }
    }

    pub fn set_active(&mut self, id: &str) -> bool {
        if self.data.provider_profiles.iter().any(|p| p.id == id) {
            self.data.active_provider_id = id.to_string();
            true
        } else {
            false
        }
    }
}

pub fn parse_model_list(profile: &ProviderProfile) -> Vec<String> {
    let mut models: Vec<String> = profile
        .model_list
        .split(['\n', '\r', ','])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if !profile.model.is_empty() && !models.contains(&profile.model) {
        models.insert(0, profile.model.clone());
    }
    models
}
