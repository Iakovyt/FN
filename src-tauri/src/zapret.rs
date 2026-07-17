use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::error::{AppError, AppResult};
use crate::github;
use crate::proc;
use crate::state::AppState;
use crate::strategies::{self, ZapretPaths};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallStatus {
    pub installed: bool,
    pub stage: String,
}

const MODULE: &str = "zapret";

// ---- Path resolution ----------------------------------------------------

fn candidate_roots(app: &AppHandle) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(root) = configured_root(app) {
        roots.push(root);
    }
    if let Ok(data) = crate::config::data_dir(app) {
        push_unique(&mut roots, data.join("zapret"));
    }
    if let Ok(res) = app.path().resource_dir() {
        push_unique(&mut roots, res.join("resources").join("zapret"));
    }
    roots
}

fn configured_root(app: &AppHandle) -> Option<PathBuf> {
    app.try_state::<AppState>()
        .and_then(|state| state.config.lock().unwrap().zapret.folder_path.clone())
        .filter(|path| !path.trim().is_empty())
        .map(PathBuf::from)
}

fn push_unique(roots: &mut Vec<PathBuf>, root: PathBuf) {
    if !roots.iter().any(|existing| existing == &root) {
        roots.push(root);
    }
}

fn packaged_root(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .resource_dir()
        .ok()
        .map(|root| root.join("resources").join("zapret"))
        .filter(|root| winws_in(root).is_some())
}

fn winws_in(root: &Path) -> Option<PathBuf> {
    for p in [root.join("bin").join("winws.exe"), root.join("winws.exe")] {
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// The install target when nothing is present yet: `%APPDATA%\FN\zapret`.
pub fn user_root(app: &AppHandle) -> AppResult<PathBuf> {
    Ok(crate::config::data_dir(app)?.join("zapret"))
}

fn install_root(app: &AppHandle) -> AppResult<PathBuf> {
    user_root(app)
}

/// Resolve the active zapret layout, preferring an already-installed copy.
pub fn resolve_paths(app: &AppHandle) -> AppResult<ZapretPaths> {
    for root in candidate_roots(app) {
        if let Some(winws) = winws_in(&root) {
            let bin = winws.parent().unwrap_or(&root).to_path_buf();
            let lists = pick_lists_dir(&root);
            return Ok(ZapretPaths { root, bin, lists });
        }
    }
    let root = install_root(app)?;
    let bin = root.join("bin");
    let lists = root.join("lists");
    Ok(ZapretPaths { root, bin, lists })
}

/// Validate a user-selected Zapret folder and resolve its layout.
pub fn paths_for_root(root: PathBuf) -> AppResult<ZapretPaths> {
    let root = std::fs::canonicalize(root)?;
    let winws = winws_in(&root)
        .ok_or_else(|| AppError::Msg("В выбранной папке не найден bin\\winws.exe".into()))?;
    let bin = winws.parent().unwrap_or(&root).to_path_buf();
    let lists = pick_lists_dir(&root);
    Ok(ZapretPaths { root, bin, lists })
}

fn pick_lists_dir(root: &PathBuf) -> PathBuf {
    let lists = root.join("lists");
    if lists.is_dir() {
        lists
    } else {
        root.to_path_buf()
    }
}

pub fn is_installed(app: &AppHandle) -> bool {
    candidate_roots(app).iter().any(|r| winws_in(r).is_some())
}

// ---- Install / bootstrap ------------------------------------------------

pub async fn ensure_installed(app: &AppHandle) -> AppResult<InstallStatus> {
    if configured_root(app).is_some_and(|root| winws_in(&root).is_some()) {
        return Ok(InstallStatus {
            installed: true,
            stage: "выбранная папка готова".into(),
        });
    }
    let destination = install_root(app)?;
    if winws_in(&destination).is_some() {
        return Ok(InstallStatus {
            installed: true,
            stage: "готово".into(),
        });
    }

    if let Some(source) = packaged_root(app) {
        proc::log(app, MODULE, "Подготовка встроенного комплекта Запрета…");
        let destination_clone = destination.clone();
        tokio::task::spawn_blocking(move || {
            crate::config::copy_dir_all(&source, &destination_clone)
        })
        .await
        .map_err(|e| AppError::Msg(e.to_string()))??;

        if winws_in(&destination).is_some() {
            proc::log(app, MODULE, "Встроенный комплект Запрета готов");
            return Ok(InstallStatus {
                installed: true,
                stage: "встроенный комплект готов".into(),
            });
        }
    }

    proc::log(
        app,
        MODULE,
        "Бинарники не найдены — загрузка из резервного зеркала…",
    );
    let (tag, bytes) = github::download_latest_zapret().await?;
    proc::log(app, MODULE, format!("Загрузка релиза {tag}…"));

    let dest = destination;
    proc::log(app, MODULE, "Распаковка…");
    let dest_clone = dest.clone();
    tokio::task::spawn_blocking(move || github::extract_zip(&bytes, &dest_clone))
        .await
        .map_err(|e| AppError::Msg(e.to_string()))??;

    if !is_installed(app) {
        return Err(AppError::BinaryMissing("winws.exe".into()));
    }

    if let Some(state) = app.try_state::<AppState>() {
        *state.zapret_tag.lock().unwrap() = Some(tag.clone());
    }
    proc::log(app, MODULE, format!("Установлен zapret {tag}"));
    Ok(InstallStatus {
        installed: true,
        stage: format!("установлено ({tag})"),
    })
}

// ---- Start / stop -------------------------------------------------------

pub async fn start(app: &AppHandle, strategy_id: &str) -> AppResult<()> {
    ensure_installed(app).await?;
    let paths = resolve_paths(app)?;
    if winws_in(&paths.root).is_none() {
        return Err(AppError::BinaryMissing("winws.exe".into()));
    }
    ensure_user_lists(&paths)?;

    // Stop any previous instance first (restart semantics).
    stop(app);

    let gaming = {
        let state = app.state::<AppState>();
        let cfg = state.config.lock().unwrap();
        cfg.zapret.gaming_mode
    };

    if strategy_id == strategies::AUTO_ID {
        auto_probe(app, &paths, gaming).await?;
    } else {
        spawn_selected_strategy(app, &paths, strategy_id, gaming)?;
        confirm_started(app).await?;
    }
    proc::emit_status(app, MODULE, true);
    monitor_exit(app.clone());
    Ok(())
}

pub fn stop(app: &AppHandle) {
    let state = app.state::<AppState>();
    state.zapret.lock().unwrap().stop();
    proc::kill_image("winws.exe");
    *state.active_strategy.lock().unwrap() = None;
    proc::emit_status(app, MODULE, false);
}

fn spawn_selected_strategy(
    app: &AppHandle,
    paths: &ZapretPaths,
    strategy_id: &str,
    gaming: bool,
) -> AppResult<()> {
    let args = if strategies::is_batch(strategy_id) {
        strategies::build_batch_args(strategy_id, paths)?
    } else {
        strategies::build_args(strategy_id, paths, gaming)
    };
    let mut cmd = Command::new(paths.winws());
    cmd.args(&args)
        .current_dir(&paths.root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(proc::CREATE_NO_WINDOW);
    }

    proc::log(app, MODULE, format!("Запуск стратегии «{strategy_id}»"));
    let mut child = cmd.spawn().map_err(|e| {
        AppError::Msg(format!(
            "не удалось запустить winws.exe (нужны права администратора?): {e}"
        ))
    })?;
    proc::attach_logs(app, MODULE, &mut child);

    let state = app.state::<AppState>();
    state.zapret.lock().unwrap().child = Some(child);
    *state.active_strategy.lock().unwrap() = Some(strategy_id.to_string());
    Ok(())
}

fn ensure_user_lists(paths: &ZapretPaths) -> AppResult<()> {
    std::fs::create_dir_all(&paths.lists)?;
    for (name, contents) in [
        ("ipset-exclude-user.txt", "203.0.113.113/32\n"),
        (
            "list-general-user.txt",
            "# Never leave this file empty\ndomain.example.abc\n",
        ),
        ("list-exclude-user.txt", "domain.example.abc\n"),
    ] {
        let path = paths.lists.join(name);
        if !path.exists() {
            std::fs::write(path, contents)?;
        }
    }
    Ok(())
}

async fn confirm_started(app: &AppHandle) -> AppResult<()> {
    tokio::time::sleep(Duration::from_millis(800)).await;
    if app.state::<AppState>().zapret_running() {
        return Ok(());
    }
    *app.state::<AppState>().active_strategy.lock().unwrap() = None;
    let detail = app
        .state::<AppState>()
        .zapret_log
        .snapshot()
        .last()
        .cloned()
        .unwrap_or_else(|| "winws.exe завершился сразу после запуска".into());
    Err(AppError::Msg(format!("Запрет не запустился: {detail}")))
}

fn monitor_exit(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if app.state::<AppState>().zapret_running() {
                continue;
            }
            *app.state::<AppState>().active_strategy.lock().unwrap() = None;
            {
                let state = app.state::<AppState>();
                state.config.lock().unwrap().zapret.enabled = false;
                let cfg = state.config.lock().unwrap().clone();
                let _ = crate::config::save(&app, &cfg);
            }
            proc::log(&app, MODULE, "Процесс winws.exe остановлен");
            proc::emit_status(&app, MODULE, false);
            break;
        }
    });
}

/// Try each strategy in turn, keeping the first that passes the health check.
async fn auto_probe(app: &AppHandle, paths: &ZapretPaths, gaming: bool) -> AppResult<()> {
    proc::log(app, MODULE, "Авто-подбор стратегии…");
    for id in strategies::PROBE_ORDER {
        proc::log(app, MODULE, format!("Проверка «{id}»…"));
        spawn_selected_strategy(app, paths, id, gaming)?;
        // Give winws a moment to install its filters.
        tokio::time::sleep(Duration::from_millis(1500)).await;

        // Still alive?
        if !app.state::<AppState>().zapret_running() {
            proc::log(app, MODULE, format!("«{id}» завершился преждевременно"));
            continue;
        }

        if health_check().await {
            proc::log(app, MODULE, format!("Рабочая стратегия: «{id}»"));
            return Ok(());
        }

        proc::log(app, MODULE, format!("«{id}» не прошла проверку"));
        stop(app);
    }
    Err(AppError::NoWorkingStrategy)
}

/// Probe real connectivity through the bypass to a commonly-blocked host.
async fn health_check() -> bool {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(4))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    for url in ["https://discord.com", "https://www.youtube.com"] {
        if client.head(url).send().await.is_ok() {
            return true;
        }
    }
    false
}

// ---- IPset --------------------------------------------------------------

fn ipset_file(paths: &ZapretPaths) -> PathBuf {
    let candidate = paths.lists.join("ipset-discord.txt");
    if candidate.exists() {
        candidate
    } else {
        // Create it under lists/ so future starts pick it up.
        candidate
    }
}

/// Append an IP/CIDR to the active ipset file and hot-restart zapret so the
/// new range takes effect. The entry is assumed pre-validated by the frontend.
pub async fn add_ipset_entry(app: &AppHandle, entry: &str) -> AppResult<()> {
    let paths = resolve_paths(app)?;
    std::fs::create_dir_all(&paths.lists)?;
    let file = ipset_file(&paths);

    // Skip duplicates.
    let existing = std::fs::read_to_string(&file).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == entry) {
        proc::log(app, MODULE, format!("{entry} уже в IPset"));
        return Ok(());
    }

    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)?;
    if !existing.is_empty() && !existing.ends_with('\n') {
        writeln!(f)?;
    }
    writeln!(f, "{entry}")?;
    proc::log(app, MODULE, format!("IPset += {entry}"));

    // Soft restart so winws re-reads the ipset (it loads the file at startup).
    if app.state::<AppState>().zapret_running() {
        let active = app
            .state::<AppState>()
            .active_strategy
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| strategies::PROBE_ORDER[0].to_string());
        let gaming = app
            .state::<AppState>()
            .config
            .lock()
            .unwrap()
            .zapret
            .gaming_mode;
        stop(app);
        spawn_selected_strategy(app, &paths, &active, gaming)?;
        proc::emit_status(app, MODULE, true);
        proc::log(app, MODULE, "IPset перезагружен");
    }
    Ok(())
}

/// Append a user domain to the main general host list.
pub async fn add_general_domain(app: &AppHandle, domain: &str) -> AppResult<()> {
    let paths = resolve_paths(app)?;
    ensure_user_lists(&paths)?;
    let file = paths.lists.join("list-general.txt");

    let existing = std::fs::read_to_string(&file).unwrap_or_default();
    if existing
        .lines()
        .any(|line| line.trim().eq_ignore_ascii_case(domain))
    {
        proc::log(app, MODULE, format!("{domain} уже в list-general.txt"));
        return Ok(());
    }

    use std::io::Write;
    let mut output = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)?;
    if !existing.is_empty() && !existing.ends_with('\n') {
        writeln!(output)?;
    }
    writeln!(output, "{domain}")?;
    proc::log(app, MODULE, format!("list-general.txt += {domain}"));

    if app.state::<AppState>().zapret_running() {
        let active = app
            .state::<AppState>()
            .active_strategy
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| strategies::PROBE_ORDER[0].to_string());
        let gaming = app
            .state::<AppState>()
            .config
            .lock()
            .unwrap()
            .zapret
            .gaming_mode;
        stop(app);
        spawn_selected_strategy(app, &paths, &active, gaming)?;
        confirm_started(app).await?;
        proc::emit_status(app, MODULE, true);
        proc::log(app, MODULE, "list-general.txt применён");
    }
    Ok(())
}

// ---- Auto-update of strategy lists / ipsets -----------------------------

/// Re-download the release if a newer tag is available. Returns true if updated.
pub async fn update_lists(app: &AppHandle) -> AppResult<bool> {
    let (latest, bytes) = github::download_latest_zapret().await?;
    let current = app
        .try_state::<AppState>()
        .and_then(|s| s.zapret_tag.lock().unwrap().clone());

    if current.as_deref() == Some(latest.as_str()) {
        return Ok(false);
    }

    proc::log(
        app,
        MODULE,
        format!("Доступно обновление списков: {latest}"),
    );
    let tag = latest;
    let dest = install_root(app)?;
    let dest_clone = dest.clone();
    tokio::task::spawn_blocking(move || github::extract_zip(&bytes, &dest_clone))
        .await
        .map_err(|e| AppError::Msg(e.to_string()))??;

    if let Some(state) = app.try_state::<AppState>() {
        *state.zapret_tag.lock().unwrap() = Some(tag);
    }
    proc::log(app, MODULE, "Списки обновлены");
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_user_lists_are_created_before_launch() {
        let unique = format!(
            "fn-zapret-user-lists-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        let paths = ZapretPaths {
            bin: root.join("bin"),
            lists: root.join("lists"),
            root: root.clone(),
        };

        ensure_user_lists(&paths).expect("create user lists");

        for name in [
            "ipset-exclude-user.txt",
            "list-general-user.txt",
            "list-exclude-user.txt",
        ] {
            assert!(paths.lists.join(name).is_file(), "missing {name}");
        }
        std::fs::remove_dir_all(root).expect("cleanup test directory");
    }
}
