use std::io::{BufRead, BufReader, Read};
use std::process::Child;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;

/// Windows: don't pop up a console window for the child process.
#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[cfg(windows)]
pub fn kill_image(image: &str) {
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("taskkill.exe")
        .args(["/F", "/IM", image])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
}

#[cfg(not(windows))]
pub fn kill_image(_image: &str) {}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LogLine {
    module: &'static str,
    line: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModuleStatus {
    module: &'static str,
    running: bool,
}

pub fn emit_status(app: &AppHandle, module: &'static str, running: bool) {
    let _ = app.emit("module-status", ModuleStatus { module, running });
}

pub fn log(app: &AppHandle, module: &'static str, line: impl Into<String>) {
    let line = line.into();
    if let Some(state) = app.try_state::<AppState>() {
        match module {
            "zapret" => state.zapret_log.push(line.clone()),
            "tgws" => state.tgws_log.push(line.clone()),
            _ => {}
        }
    }
    let _ = app.emit("log-line", LogLine { module, line });
}

/// Take stdout+stderr off a freshly spawned child and stream every line to the
/// UI (and the module's ring buffer) on background threads.
pub fn attach_logs(app: &AppHandle, module: &'static str, child: &mut Child) {
    if let Some(out) = child.stdout.take() {
        spawn_reader(app.clone(), module, out);
    }
    if let Some(err) = child.stderr.take() {
        spawn_reader(app.clone(), module, err);
    }
}

fn spawn_reader<R: Read + Send + 'static>(app: AppHandle, module: &'static str, stream: R) {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        loop {
            let mut bytes = Vec::new();
            match reader.read_until(b'\n', &mut bytes) {
                Ok(0) => break,
                Ok(_) => {
                    while bytes
                        .last()
                        .is_some_and(|byte| matches!(byte, b'\r' | b'\n'))
                    {
                        bytes.pop();
                    }
                    let line = String::from_utf8_lossy(&bytes).trim().to_string();
                    if !line.is_empty() {
                        log(&app, module, line);
                    }
                }
                Err(_) => break,
            }
        }
    });
}
