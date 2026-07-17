use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::error::AppResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ZapretConfig {
    pub strategy_id: String,
    pub folder_path: Option<String>,
    pub gaming_mode: bool,
    pub auto_update: bool,
    pub auto_ipset: bool,
    pub enabled: bool,
}

impl Default for ZapretConfig {
    fn default() -> Self {
        Self {
            strategy_id: "auto".into(),
            folder_path: None,
            gaming_mode: false,
            auto_update: true,
            auto_ipset: true,
            enabled: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TgwsConfig {
    pub host: String,
    pub port: u16,
    pub secret: String,
    pub enabled: bool,
}

impl Default for TgwsConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 2222,
            secret: String::new(),
            enabled: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppConfig {
    pub zapret: ZapretConfig,
    pub tgws: TgwsConfig,
}

/// Root data directory for FN: `%APPDATA%\FN`.
///
/// Note: the spec text mentions `%APPDATA%/NetShield`, but the rename brief
/// says the new name "FN" must be used everywhere including configs, so we
/// keep everything under `FN` for consistency.
pub fn data_dir(app: &AppHandle) -> AppResult<PathBuf> {
    let base = app
        .path()
        .config_dir()
        .map_err(|e| crate::error::AppError::Msg(e.to_string()))?;
    let dir = base.join("FN");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn config_path(app: &AppHandle) -> AppResult<PathBuf> {
    Ok(data_dir(app)?.join("config.json"))
}

pub fn load(app: &AppHandle) -> AppConfig {
    match config_path(app).and_then(|p| Ok(std::fs::read_to_string(p)?)) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => AppConfig::default(),
    }
}

pub fn save(app: &AppHandle, cfg: &AppConfig) -> AppResult<()> {
    let path = config_path(app)?;
    let text = serde_json::to_string_pretty(cfg)?;
    std::fs::write(path, text)?;
    Ok(())
}

pub fn copy_dir_all(source: &std::path::Path, destination: &std::path::Path) -> AppResult<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_dir_all(&source_path, &destination_path)?;
        } else {
            std::fs::copy(&source_path, &destination_path)?;
        }
    }
    Ok(())
}
