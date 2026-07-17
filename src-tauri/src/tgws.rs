use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use tauri::{AppHandle, Manager};
use tauri_plugin_shell::ShellExt;

use crate::error::{AppError, AppResult};
use crate::proc;
use crate::state::AppState;

const MODULE: &str = "tgws";
const HEADLESS_BINARY: &str = "TgWsProxy_headless.exe";
const LEGACY_TRAY_BINARY: &str = "TgWsProxy_windows.exe";

// ---- Path resolution ----------------------------------------------------

fn packaged_root(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .resource_dir()
        .ok()
        .map(|root| root.join("resources").join("tgws"))
        .filter(|root| find_exe(root).is_some())
}

fn install_root(app: &AppHandle) -> AppResult<PathBuf> {
    Ok(crate::config::data_dir(app)?.join("tgws"))
}

fn find_exe(root: &Path) -> Option<PathBuf> {
    let executable = root.join(HEADLESS_BINARY);
    executable.exists().then_some(executable)
}

// ---- Install ------------------------------------------------------------

pub async fn ensure_installed(app: &AppHandle) -> AppResult<PathBuf> {
    let destination = install_root(app)?;
    if let Some(bin) = find_exe(&destination) {
        return Ok(bin);
    }

    if let Some(source) = packaged_root(app) {
        let destination_clone = destination.clone();
        tokio::task::spawn_blocking(move || {
            crate::config::copy_dir_all(&source, &destination_clone)
        })
        .await
        .map_err(|e| AppError::Msg(e.to_string()))??;
        if let Some(bin) = find_exe(&destination) {
            proc::log(app, MODULE, "Встроенный TGWS готов");
            return Ok(bin);
        }
    }

    Err(AppError::BinaryMissing(HEADLESS_BINARY.into()))
}

fn valid_secret(secret: &str) -> bool {
    secret.len() == 32 && secret.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn fresh_secret() -> AppResult<String> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|error| AppError::Msg(format!("не удалось создать TGWS secret: {error}")))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub fn ensure_secret(app: &AppHandle) -> AppResult<String> {
    let state = app.state::<AppState>();
    let mut config = state.config.lock().unwrap();
    if valid_secret(&config.tgws.secret) {
        return Ok(config.tgws.secret.to_ascii_lowercase());
    }

    let secret = fresh_secret()?;
    config.tgws.secret.clone_from(&secret);
    let snapshot = config.clone();
    drop(config);
    crate::config::save(app, &snapshot)?;
    Ok(secret)
}

pub fn refresh_secret(app: &AppHandle) -> AppResult<String> {
    let secret = fresh_secret()?;
    let state = app.state::<AppState>();
    let mut config = state.config.lock().unwrap();
    config.tgws.secret.clone_from(&secret);
    let snapshot = config.clone();
    drop(config);
    crate::config::save(app, &snapshot)?;
    Ok(secret)
}

// ---- Start / stop -------------------------------------------------------

pub async fn start(app: &AppHandle) -> AppResult<()> {
    let bin = ensure_installed(app).await?;
    stop(app); // restart semantics

    let secret = ensure_secret(app)?;
    let (host, port) = {
        let state = app.state::<AppState>();
        let cfg = state.config.lock().unwrap();
        (cfg.tgws.host.clone(), cfg.tgws.port)
    };
    let port_text = port.to_string();

    let mut cmd = Command::new(&bin);
    cmd.current_dir(bin.parent().unwrap_or(&bin))
        .args([
            "--host",
            &host,
            "--port",
            &port_text,
            "--secret",
            &secret,
            "--dc-ip",
            "2:149.154.167.220",
            "--dc-ip",
            "4:149.154.167.220",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(proc::CREATE_NO_WINDOW);
    }

    proc::log(app, MODULE, format!("Запуск прокси на {host}:{port}"));
    let mut child = cmd
        .spawn()
        .map_err(|e| AppError::Msg(format!("не удалось запустить tg-ws-proxy: {e}")))?;
    proc::attach_logs(app, MODULE, &mut child);

    app.state::<AppState>().tgws.lock().unwrap().child = Some(child);
    confirm_listening(app, &host, port).await?;
    proc::emit_status(app, MODULE, true);
    monitor_exit(app.clone());
    Ok(())
}

pub fn stop(app: &AppHandle) {
    let state = app.state::<AppState>();
    state.tgws.lock().unwrap().stop();
    proc::kill_image(HEADLESS_BINARY);
    proc::kill_image(LEGACY_TRAY_BINARY);
    proc::emit_status(app, MODULE, false);
}

async fn confirm_listening(app: &AppHandle, host: &str, port: u16) -> AppResult<()> {
    let connect_host = match host {
        "0.0.0.0" | "::" => "127.0.0.1",
        value => value,
    };
    // The one-file executable can need extra time on its first launch while
    // Windows scans and extracts it.
    for _ in 0..90 {
        if !app.state::<AppState>().tgws_running() {
            let detail = proxy_log_tail(app)
                .unwrap_or_else(|| "процесс TGWS завершился сразу после запуска".to_string());
            return Err(AppError::Msg(format!("TGWS не запустился: {detail}")));
        }
        let attempt = tokio::net::TcpStream::connect((connect_host, port));
        if tokio::time::timeout(Duration::from_millis(250), attempt)
            .await
            .is_ok_and(|result| result.is_ok())
        {
            proc::log(app, MODULE, format!("TGWS слушает {host}:{port}"));
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    stop(app);
    let detail = proxy_log_tail(app).unwrap_or_else(|| "порт не открылся".into());
    Err(AppError::Msg(format!(
        "TGWS не открыл {host}:{port}: {detail}"
    )))
}

fn proxy_log_tail(app: &AppHandle) -> Option<String> {
    app.state::<AppState>().tgws_log.snapshot().last().cloned()
}

fn monitor_exit(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if app.state::<AppState>().tgws_running() {
                continue;
            }
            {
                let state = app.state::<AppState>();
                state.config.lock().unwrap().tgws.enabled = false;
                let cfg = state.config.lock().unwrap().clone();
                let _ = crate::config::save(&app, &cfg);
            }
            proc::log(&app, MODULE, "Процесс TGWS остановлен");
            proc::emit_status(&app, MODULE, false);
            break;
        }
    });
}

// ---- Telegram deep link -------------------------------------------------

fn telegram_links(host: &str, port: u16, secret: &str) -> (String, String) {
    // A different loopback spelling makes Telegram replace a cached proxy
    // entry that may still contain the old secret.
    let link_host = if host == "127.0.0.1" {
        "localhost"
    } else {
        host
    };
    let secret = secret.strip_prefix("dd").unwrap_or(secret);
    let query = format!("server={link_host}&port={port}&secret=dd{secret}");
    (
        format!("tg://proxy?{query}"),
        format!("https://t.me/proxy?{query}"),
    )
}

pub async fn open_telegram(app: &AppHandle) -> AppResult<()> {
    let secret = ensure_secret(app)?;
    let (host, port) = {
        let state = app.state::<AppState>();
        let cfg = state.config.lock().unwrap();
        (cfg.tgws.host.clone(), cfg.tgws.port)
    };
    let (tg, web) = telegram_links(&host, port, &secret);

    match app.shell().open(&tg, None) {
        Ok(_) => Ok(()),
        Err(_) => app
            .shell()
            .open(&web, None)
            .map_err(|e| AppError::Msg(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_exactly_16_bytes_of_hex() {
        assert!(valid_secret("0123456789abcdef0123456789abcdef"));
        assert!(!valid_secret("0123456789abcdef"));
        assert!(!valid_secret("0123456789abcdef0123456789abcdeg"));
    }

    #[test]
    fn telegram_link_uses_mtproto_prefix_and_new_loopback_name() {
        let (native, web) = telegram_links("127.0.0.1", 2222, "0123456789abcdef0123456789abcdef");
        let query = "server=localhost&port=2222&secret=dd0123456789abcdef0123456789abcdef";
        assert_eq!(native, format!("tg://proxy?{query}"));
        assert_eq!(web, format!("https://t.me/proxy?{query}"));
    }
}
