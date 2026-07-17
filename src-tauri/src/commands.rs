use std::net::IpAddr;
use std::str::FromStr;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::config::AppConfig;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::strategies::{self, StrategyInfo};
use crate::tgws;
use crate::zapret::{self, InstallStatus};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub active_modules: u32,
    pub traffic_bytes_per_sec: u64,
    pub uptime_secs: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderSelection {
    pub folder_path: String,
    pub strategy_id: String,
    pub strategies: Vec<StrategyInfo>,
}

fn save_config(app: &AppHandle, state: &AppState) -> AppResult<()> {
    let cfg = state.config.lock().unwrap().clone();
    crate::config::save(app, &cfg)
}

// ---- Config -------------------------------------------------------------

#[tauri::command]
pub fn get_config(state: State<AppState>) -> AppConfig {
    state.config.lock().unwrap().clone()
}

#[tauri::command]
pub fn get_autostart() -> bool {
    crate::autostart::is_enabled()
}

#[tauri::command]
pub fn set_autostart(enabled: bool) -> AppResult<()> {
    crate::autostart::set_enabled(enabled)
}

// ---- Zapret -------------------------------------------------------------

#[tauri::command]
pub fn list_strategies(app: AppHandle) -> AppResult<Vec<StrategyInfo>> {
    let paths = zapret::resolve_paths(&app)?;
    strategies::list_for_root(&paths.root)
}

#[tauri::command]
pub async fn ensure_zapret_installed(app: AppHandle) -> AppResult<InstallStatus> {
    zapret::ensure_installed(&app).await
}

#[tauri::command]
pub fn set_zapret_folder(app: AppHandle, path: String) -> AppResult<FolderSelection> {
    let paths = zapret::paths_for_root(path.into())?;
    let available = strategies::list_for_root(&paths.root)?;
    if !available
        .iter()
        .any(|strategy| strategies::is_batch(&strategy.id))
    {
        return Err(AppError::Msg(
            "В выбранной папке не найдено запускаемых BAT-стратегий".into(),
        ));
    }

    if app.state::<AppState>().zapret_running() {
        zapret::stop(&app);
    }

    let folder_path = paths.root.to_string_lossy().into_owned();
    let strategy_id = {
        let state = app.state::<AppState>();
        let mut cfg = state.config.lock().unwrap();
        cfg.zapret.folder_path = Some(folder_path.clone());
        if !available
            .iter()
            .any(|strategy| strategy.id == cfg.zapret.strategy_id)
        {
            cfg.zapret.strategy_id = available
                .iter()
                .find(|strategy| !strategy.auto)
                .unwrap_or(&available[0])
                .id
                .clone();
        }
        cfg.zapret.enabled = false;
        cfg.zapret.strategy_id.clone()
    };
    save_config(&app, &app.state::<AppState>())?;

    Ok(FolderSelection {
        folder_path,
        strategy_id,
        strategies: available,
    })
}

#[tauri::command]
pub async fn zapret_start(app: AppHandle, strategy_id: String) -> AppResult<()> {
    let paths = zapret::resolve_paths(&app)?;
    if !strategies::is_known_for_root(&strategy_id, &paths.root) {
        return Err(AppError::Msg(format!(
            "неизвестная стратегия: {strategy_id}"
        )));
    }
    {
        let state = app.state::<AppState>();
        let mut cfg = state.config.lock().unwrap();
        cfg.zapret.strategy_id = strategy_id.clone();
        cfg.zapret.enabled = false;
    }
    let _ = save_config(&app, &app.state::<AppState>());
    match zapret::start(&app, &strategy_id).await {
        Ok(()) => {
            app.state::<AppState>()
                .config
                .lock()
                .unwrap()
                .zapret
                .enabled = true;
            save_config(&app, &app.state::<AppState>())
        }
        Err(error) => {
            let _ = save_config(&app, &app.state::<AppState>());
            Err(error)
        }
    }
}

#[tauri::command]
pub fn zapret_stop(app: AppHandle, state: State<AppState>) -> AppResult<()> {
    zapret::stop(&app);
    state.config.lock().unwrap().zapret.enabled = false;
    save_config(&app, &state)
}

#[tauri::command]
pub fn set_strategy(app: AppHandle, state: State<AppState>, strategy_id: String) -> AppResult<()> {
    let paths = zapret::resolve_paths(&app)?;
    if !strategies::is_known_for_root(&strategy_id, &paths.root) {
        return Err(AppError::Msg(format!(
            "неизвестная стратегия: {strategy_id}"
        )));
    }
    state.config.lock().unwrap().zapret.strategy_id = strategy_id;
    save_config(&app, &state)
}

#[tauri::command]
pub async fn set_gaming_mode(app: AppHandle, enabled: bool) -> AppResult<()> {
    let (running, strategy) = {
        let state = app.state::<AppState>();
        state.config.lock().unwrap().zapret.gaming_mode = enabled;
        let strategy = state.config.lock().unwrap().zapret.strategy_id.clone();
        (state.zapret_running(), strategy)
    };
    let _ = save_config(&app, &app.state::<AppState>());
    // Apply live by restarting the running strategy under the new mode.
    if running {
        zapret::start(&app, &strategy).await?;
    }
    Ok(())
}

#[tauri::command]
pub fn set_auto_update(app: AppHandle, state: State<AppState>, enabled: bool) -> AppResult<()> {
    state.config.lock().unwrap().zapret.auto_update = enabled;
    save_config(&app, &state)
}

#[tauri::command]
pub fn set_auto_ipset(app: AppHandle, state: State<AppState>, enabled: bool) -> AppResult<()> {
    state.config.lock().unwrap().zapret.auto_ipset = enabled;
    save_config(&app, &state)
}

#[tauri::command]
pub async fn add_ipset_entry(app: AppHandle, entry: String) -> AppResult<()> {
    let entry = entry.trim().to_string();
    if !is_valid_ip_or_cidr(&entry) {
        return Err(AppError::InvalidAddress(entry));
    }
    zapret::add_ipset_entry(&app, &entry).await
}

#[tauri::command]
pub async fn add_zapret_entry(app: AppHandle, entry: String) -> AppResult<String> {
    let entry = entry.trim();
    if is_valid_ip_or_cidr(entry) {
        zapret::add_ipset_entry(&app, entry).await?;
        return Ok("IPSet".into());
    }

    let domain =
        normalize_domain(entry).ok_or_else(|| AppError::InvalidAddress(entry.to_string()))?;
    zapret::add_general_domain(&app, &domain).await?;
    Ok("list-general.txt".into())
}

// ---- TGWS ---------------------------------------------------------------

#[tauri::command]
pub async fn tgws_start(app: AppHandle) -> AppResult<()> {
    app.state::<AppState>().config.lock().unwrap().tgws.enabled = false;
    let _ = save_config(&app, &app.state::<AppState>());
    match tgws::start(&app).await {
        Ok(()) => {
            app.state::<AppState>().config.lock().unwrap().tgws.enabled = true;
            save_config(&app, &app.state::<AppState>())
        }
        Err(error) => {
            let _ = save_config(&app, &app.state::<AppState>());
            Err(error)
        }
    }
}

#[tauri::command]
pub fn tgws_stop(app: AppHandle, state: State<AppState>) -> AppResult<()> {
    tgws::stop(&app);
    state.config.lock().unwrap().tgws.enabled = false;
    save_config(&app, &state)
}

#[tauri::command]
pub async fn set_tgws_endpoint(app: AppHandle, host: String, port: u16) -> AppResult<()> {
    let host = host.trim().to_string();
    if host.is_empty() {
        return Err(AppError::InvalidAddress("пустой хост".into()));
    }
    let running = {
        let state = app.state::<AppState>();
        let mut cfg = state.config.lock().unwrap();
        cfg.tgws.host = host;
        cfg.tgws.port = port;
        state.tgws_running()
    };
    let _ = save_config(&app, &app.state::<AppState>());
    // Restart with the new endpoint if the proxy is already running.
    if running {
        tgws_start(app.clone()).await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn open_telegram_proxy(app: AppHandle) -> AppResult<()> {
    if !app.state::<AppState>().tgws_running() {
        tgws_start(app.clone()).await?;
    }
    tgws::open_telegram(&app).await
}

#[tauri::command]
pub async fn refresh_tgws_secret(app: AppHandle) -> AppResult<String> {
    let running = app.state::<AppState>().tgws_running();
    let secret = tgws::refresh_secret(&app)?;
    if running {
        tgws_start(app.clone()).await?;
    }
    Ok(secret)
}

// ---- Stats / logs -------------------------------------------------------

#[tauri::command]
pub fn get_stats(state: State<AppState>) -> Stats {
    Stats {
        active_modules: state.active_modules(),
        traffic_bytes_per_sec: state.traffic_bps.load(std::sync::atomic::Ordering::Relaxed),
        uptime_secs: state.uptime_secs(),
    }
}

#[tauri::command]
pub fn get_logs(state: State<AppState>, module: String) -> Vec<String> {
    match module.as_str() {
        "zapret" => state.zapret_log.snapshot(),
        "tgws" => state.tgws_log.snapshot(),
        _ => Vec::new(),
    }
}

// ---- Validation ---------------------------------------------------------

fn is_valid_ip_or_cidr(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    match value.split_once('/') {
        None => IpAddr::from_str(value).is_ok(),
        Some((addr, mask)) => {
            let Ok(ip) = IpAddr::from_str(addr) else {
                return false;
            };
            let Ok(bits) = mask.parse::<u8>() else {
                return false;
            };
            match ip {
                IpAddr::V4(_) => bits <= 32,
                IpAddr::V6(_) => bits <= 128,
            }
        }
    }
}

fn normalize_domain(value: &str) -> Option<String> {
    let mut domain = value.trim().to_ascii_lowercase();
    if let Some(rest) = domain.strip_prefix("https://") {
        domain = rest.to_string();
    } else if let Some(rest) = domain.strip_prefix("http://") {
        domain = rest.to_string();
    }
    domain = domain
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .trim_start_matches("*.")
        .trim_end_matches('.')
        .to_string();

    if let Some((host, port)) = domain.rsplit_once(':') {
        if !host.contains(':') && port.parse::<u16>().is_ok() {
            domain = host.to_string();
        }
    }

    if domain.len() > 253 || !domain.contains('.') {
        return None;
    }
    let valid = domain.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    });
    valid.then_some(domain)
}

#[cfg(test)]
mod validation_tests {
    use super::*;

    #[test]
    fn normalizes_domains_and_urls() {
        assert_eq!(normalize_domain("Discord.com"), Some("discord.com".into()));
        assert_eq!(
            normalize_domain("https://www.example.com/path"),
            Some("www.example.com".into())
        );
        assert_eq!(normalize_domain("bad value"), None);
        assert_eq!(normalize_domain("localhost"), None);
    }
}
