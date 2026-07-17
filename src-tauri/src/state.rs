use std::collections::VecDeque;
use std::process::Child;
use std::sync::atomic::AtomicU64;
use std::sync::Mutex;
use std::time::Instant;

use crate::config::AppConfig;
use crate::db::Db;

const LOG_CAP: usize = 20;

/// Wraps a child process so we can check liveness and stop it cleanly.
#[derive(Default)]
pub struct ProcHandle {
    pub child: Option<Child>,
}

impl ProcHandle {
    /// True if a child exists and has not exited.
    pub fn is_running(&mut self) -> bool {
        match &mut self.child {
            Some(c) => matches!(c.try_wait(), Ok(None)),
            None => false,
        }
    }

    /// Kill the child if present and reap it. Idempotent.
    pub fn stop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// A bounded ring buffer of the most recent log lines for a module.
pub struct LogRing {
    lines: Mutex<VecDeque<String>>,
}

impl LogRing {
    fn new() -> Self {
        Self {
            lines: Mutex::new(VecDeque::with_capacity(LOG_CAP)),
        }
    }

    pub fn push(&self, line: String) {
        let mut q = self.lines.lock().unwrap();
        if q.len() == LOG_CAP {
            q.pop_front();
        }
        q.push_back(line);
    }

    pub fn snapshot(&self) -> Vec<String> {
        self.lines.lock().unwrap().iter().cloned().collect()
    }
}

/// Central runtime state, managed by Tauri and shared with background tasks
/// through `AppHandle::state()`.
pub struct AppState {
    pub config: Mutex<AppConfig>,
    pub zapret: Mutex<ProcHandle>,
    pub tgws: Mutex<ProcHandle>,
    pub zapret_log: LogRing,
    pub tgws_log: LogRing,
    /// System-wide throughput sample (bytes/sec), refreshed by the stats task.
    pub traffic_bps: AtomicU64,
    pub started_at: Instant,
    /// Release tag of the installed zapret copy, if known.
    pub zapret_tag: Mutex<Option<String>>,
    /// The concrete strategy winws is currently running (resolved from "auto").
    pub active_strategy: Mutex<Option<String>>,
    /// Hydra's strategy/history store. `None` when the DB failed to open
    /// (e.g. a locked or corrupt file) — Hydra features degrade instead of
    /// taking down the rest of the app, since zapret/tgws don't depend on it.
    pub db: Option<Db>,
}

impl AppState {
    pub fn new(config: AppConfig, db: Option<Db>) -> Self {
        Self {
            config: Mutex::new(config),
            zapret: Mutex::new(ProcHandle::default()),
            tgws: Mutex::new(ProcHandle::default()),
            zapret_log: LogRing::new(),
            tgws_log: LogRing::new(),
            traffic_bps: AtomicU64::new(0),
            started_at: Instant::now(),
            zapret_tag: Mutex::new(None),
            active_strategy: Mutex::new(None),
            db,
        }
    }

    pub fn zapret_running(&self) -> bool {
        self.zapret.lock().unwrap().is_running()
    }

    pub fn tgws_running(&self) -> bool {
        self.tgws.lock().unwrap().is_running()
    }

    pub fn active_modules(&self) -> u32 {
        (self.zapret_running() as u32) + (self.tgws_running() as u32)
    }

    pub fn uptime_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }
}
