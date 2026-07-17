mod autostart;
mod bypass;
mod commands;
mod config;
mod db;
mod error;
mod github;
mod network;
mod proc;
mod state;
mod strategies;
mod strategy;
mod tgws;
mod tray;
mod zapret;

use std::sync::atomic::Ordering;
use std::time::Duration;

use tauri::{Emitter, Manager, WindowEvent};

use crate::commands::Stats;
use crate::state::AppState;

const STATS_INTERVAL_SECS: u64 = 2;
const UPDATE_INTERVAL_SECS: u64 = 24 * 60 * 60;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let started_from_autostart =
                std::env::args_os().any(|argument| argument == "--autostart");

            // Load persisted config into shared state.
            let cfg = config::load(&handle);

            // Hydra's strategy/history DB. Missing/broken is non-fatal —
            // zapret/tgws don't depend on it, so Hydra just stays inert.
            let db = match db::open(&handle) {
                Ok(db) => {
                    if let Ok(conn) = db.conn.lock() {
                        if let Err(error) = strategy::sync_builtin_strategies(&conn) {
                            proc::log(&handle, "zapret", format!("Hydra: sync стратегий: {error}"));
                        }
                    }
                    Some(db)
                }
                Err(error) => {
                    proc::log(&handle, "zapret", format!("Hydra: БД недоступна: {error}"));
                    None
                }
            };
            app.manage(AppState::new(cfg.clone(), db));

            // System tray + menu.
            tray::build(&handle)?;

            // Close button hides to tray instead of quitting; modules keep
            // running in the background. Real exit is the tray "Выход" item.
            if let Some(win) = app.get_webview_window("main") {
                if !started_from_autostart {
                    let _ = win.show();
                    let _ = win.set_focus();
                }
                let w = win.clone();
                win.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = w.hide();
                    }
                });
            }

            spawn_stats_task(handle.clone());
            spawn_autoupdate_task(handle.clone());
            spawn_network_change_task(handle.clone());
            spawn_bootstrap_task(handle, cfg);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::get_autostart,
            commands::set_autostart,
            commands::list_strategies,
            commands::ensure_zapret_installed,
            commands::set_zapret_folder,
            commands::zapret_start,
            commands::zapret_stop,
            commands::set_strategy,
            commands::set_gaming_mode,
            commands::set_auto_update,
            commands::set_auto_ipset,
            commands::add_ipset_entry,
            commands::add_zapret_entry,
            commands::tgws_start,
            commands::tgws_stop,
            commands::set_tgws_endpoint,
            commands::open_telegram_proxy,
            commands::refresh_tgws_secret,
            commands::get_stats,
            commands::get_logs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running FN");
}

/// Emit a stats tick every couple of seconds, sampling system-wide throughput.
fn spawn_stats_task(handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut networks = sysinfo::Networks::new_with_refreshed_list();
        let mut interval = tokio::time::interval(Duration::from_secs(STATS_INTERVAL_SECS));
        loop {
            interval.tick().await;
            networks.refresh();
            let delta: u64 = networks
                .iter()
                .map(|(_, d)| d.received() + d.transmitted())
                .sum();
            let bps = delta / STATS_INTERVAL_SECS.max(1);

            let state = handle.state::<AppState>();
            state.traffic_bps.store(bps, Ordering::Relaxed);
            let stats = Stats {
                active_modules: state.active_modules(),
                traffic_bytes_per_sec: bps,
                uptime_secs: state.uptime_secs(),
            };
            let _ = handle.emit("stats", stats);
        }
    });
}

/// Once a day, refresh strategy lists / ipsets if auto-update is enabled.
fn spawn_autoupdate_task(handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(UPDATE_INTERVAL_SECS));
        interval.tick().await; // consume the immediate first tick
        loop {
            interval.tick().await;
            let auto = handle
                .state::<AppState>()
                .config
                .lock()
                .unwrap()
                .zapret
                .auto_update;
            if auto {
                let _ = zapret::update_lists(&handle).await;
            }
        }
    });
}

/// On launch: make sure binaries exist, then restore any modules that were
/// enabled when the app was last closed.
fn spawn_bootstrap_task(handle: tauri::AppHandle, cfg: config::AppConfig) {
    tauri::async_runtime::spawn(async move {
        let _ = zapret::ensure_installed(&handle).await;
        if cfg.zapret.enabled {
            if let Err(error) = zapret::start(&handle, &cfg.zapret.strategy_id).await {
                proc::log(&handle, "zapret", format!("Автозапуск: {error}"));
                handle
                    .state::<AppState>()
                    .config
                    .lock()
                    .unwrap()
                    .zapret
                    .enabled = false;
            }
        }
        if cfg.tgws.enabled {
            if let Err(error) = tgws::start(&handle).await {
                proc::log(&handle, "tgws", format!("Автозапуск: {error}"));
                handle
                    .state::<AppState>()
                    .config
                    .lock()
                    .unwrap()
                    .tgws
                    .enabled = false;
            }
        }
        let current = handle.state::<AppState>().config.lock().unwrap().clone();
        let _ = config::save(&handle, &current);

        // First-run trigger: only after the legacy zapret auto-start attempt
        // above has resolved, so `zapret_running()` reflects reality instead
        // of racing it (`run_benchmark` refuses to start if zapret owns the
        // winws process already).
        if !handle.state::<AppState>().zapret_running() {
            let has_active = handle
                .state::<AppState>()
                .db
                .as_ref()
                .and_then(|db| db.conn.lock().ok().and_then(|conn| strategy::has_active_strategy(&conn).ok()))
                .unwrap_or(true); // DB unavailable: don't guess, stay inert
            if !has_active {
                trigger_hydra_rescan(&handle).await;
            }
        }
    });
}

const NETWORK_POLL_INTERVAL_SECS: u64 = 20;

/// Polls the "primary local IP" (see `network.rs`) and re-runs Hydra's
/// benchmark whenever it changes — the network-change trigger from the
/// task spec, implemented without the `windows` crate (see `network.rs`
/// for why).
fn spawn_network_change_task(handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut last_ip = tokio::task::spawn_blocking(network::primary_local_ip)
            .await
            .ok()
            .flatten();
        let mut interval = tokio::time::interval(Duration::from_secs(NETWORK_POLL_INTERVAL_SECS));
        interval.tick().await; // consume the immediate first tick
        loop {
            interval.tick().await;
            let current = tokio::task::spawn_blocking(network::primary_local_ip)
                .await
                .ok()
                .flatten();
            if current != last_ip {
                proc::log(
                    &handle,
                    "zapret",
                    "Hydra: обнаружена смена сети, пересканирование стратегий",
                );
                last_ip = current;
                trigger_hydra_rescan(&handle).await;
            }
        }
    });
}

/// Loads the current strategy pool and runs Hydra's benchmark against the
/// default targets, logging the outcome. Shared by the first-run and
/// network-change triggers; the manual trigger (`trigger_rescan` IPC, Stage
/// 4) will call `strategy::benchmark::run_benchmark` the same way.
async fn trigger_hydra_rescan(handle: &tauri::AppHandle) {
    let state = handle.state::<AppState>();
    let Some(db) = state.db.as_ref() else {
        return;
    };
    let pool = {
        let Ok(conn) = db.conn.lock() else {
            return;
        };
        strategy::load_pool(&conn, None).unwrap_or_default()
    };
    if pool.is_empty() {
        return;
    }

    match strategy::benchmark::run_benchmark(handle, &pool, strategy::benchmark::DEFAULT_TARGETS)
        .await
    {
        Ok(strategy) => proc::log(
            handle,
            "zapret",
            format!("Hydra: активирована стратегия «{}»", strategy.name),
        ),
        Err(error) => proc::log(handle, "zapret", format!("Hydra: автоподбор не удался: {error}")),
    }
}
