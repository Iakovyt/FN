//! Spawns and tears down a `winws.exe` child process. This is the one
//! place that builds the `Command` — extracted out of
//! `zapret.rs::spawn_selected_strategy`, which now calls [`spawn_child`]
//! too, so there's a single spawn implementation instead of two copies
//! (one for the classic zapret UI, one for Hydra's benchmark runner).

use std::path::Path;
use std::process::{Child, Command, Stdio};

use tauri::AppHandle;

use crate::error::{AppError, AppResult};
use crate::proc;
use crate::state::ProcHandle;

/// Launches `winws_path args...` in `cwd` with stdout/stderr piped into the
/// app's log ring (via `proc::attach_logs`) and no console window on
/// Windows. Returns the raw [`Child`] so existing call sites (e.g.
/// `zapret.rs`, which stores it straight into `AppState.zapret.child`) don't
/// need to change shape.
pub fn spawn_child(
    app: &AppHandle,
    module: &'static str,
    winws_path: &Path,
    args: &[String],
    cwd: &Path,
) -> AppResult<Child> {
    let mut cmd = Command::new(winws_path);
    cmd.args(args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(proc::CREATE_NO_WINDOW);
    }

    let mut child = cmd.spawn().map_err(|e| {
        AppError::Msg(format!(
            "не удалось запустить winws.exe (нужны права администратора?): {e}"
        ))
    })?;
    proc::attach_logs(app, module, &mut child);
    Ok(child)
}

/// A standalone winws process Hydra owns end-to-end (benchmark probes,
/// eventually the watchdog in Stage 3) — as opposed to `AppState.zapret`,
/// which the classic manual-strategy UI owns. Keeping these separate means
/// Hydra can spawn/kill its own process without touching the legacy
/// control surface (`zapret_stop`, `set_gaming_mode`, ...).
pub struct WinwsHandle {
    proc: ProcHandle,
}

impl WinwsHandle {
    pub fn spawn(
        app: &AppHandle,
        module: &'static str,
        winws_path: &Path,
        args: &[String],
        cwd: &Path,
    ) -> AppResult<Self> {
        let child = spawn_child(app, module, winws_path, args, cwd)?;
        Ok(Self {
            proc: ProcHandle { child: Some(child) },
        })
    }

    pub fn is_running(&mut self) -> bool {
        self.proc.is_running()
    }

    /// Kills the process and reaps it. Consumes `self` — a torn-down
    /// handle has nothing left to hand back to the caller.
    pub fn teardown(mut self) {
        self.proc.stop();
    }

    /// Unwraps into the underlying [`ProcHandle`] so the caller can move it
    /// into a shared `Mutex<ProcHandle>` slot (e.g. `AppState.hydra`) once
    /// a benchmark winner gets promoted to production.
    pub fn into_inner(self) -> ProcHandle {
        self.proc
    }
}
