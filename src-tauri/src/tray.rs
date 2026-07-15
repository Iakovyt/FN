use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::state::AppState;
use crate::{tgws, zapret};

/// Bring the main window back to the foreground.
fn show_main(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItemBuilder::with_id("open", "Открыть").build(app)?;
    let zapret = MenuItemBuilder::with_id("toggle_zapret", "Запрет: вкл/выкл").build(app)?;
    let tgws = MenuItemBuilder::with_id("toggle_tgws", "TGWS: вкл/выкл").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Выход").build(app)?;

    let menu = MenuBuilder::new(app)
        .item(&open)
        .separator()
        .item(&zapret)
        .item(&tgws)
        .separator()
        .item(&quit)
        .build()?;

    let mut builder = TrayIconBuilder::with_id("fn-tray")
        .tooltip("FN")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app),
            "toggle_zapret" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let (running, strategy) = {
                        let state = app.state::<AppState>();
                        let running = state.zapret_running();
                        // Bind to a local so the MutexGuard drops before `state`.
                        let strategy = state.config.lock().unwrap().zapret.strategy_id.clone();
                        (running, strategy)
                    };
                    if running {
                        zapret::stop(&app);
                    } else {
                        let _ = zapret::start(&app, &strategy).await;
                    }
                });
            }
            "toggle_tgws" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    if app.state::<AppState>().tgws_running() {
                        tgws::stop(&app);
                    } else {
                        let _ = tgws::start(&app).await;
                    }
                });
            }
            "quit" => {
                // The only real exit path: stop modules, then terminate.
                zapret::stop(app);
                tgws::stop(app);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app)?;
    Ok(())
}
