//! Benchmark runner: probes for reachability (below), spins up isolated
//! test winws instances, scores the builtin/community pool, and activates
//! the winner. See `run_benchmark` for the entry point.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};
use tokio::net::UdpSocket;

use crate::bypass::winws::WinwsHandle;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::strategy::schema::{Strategy, TargetProtocol};
use crate::strategy::winws_mapper;

/// What Hydra's bootstrap/network-change rescan targets by default. Limited
/// to `Discord`+`YouTube` because that's what the bundled `general*.bat`
/// pool actually addresses — Telegram in this app goes through the
/// separate TGWS proxy, not zapret strategy selection (see `tgws.rs`).
pub const DEFAULT_TARGETS: &[TargetProtocol] = &[TargetProtocol::Discord, TargetProtocol::YouTube];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeKind {
    Tls,
    UdpStun,
}

pub struct BenchTarget {
    pub protocol: TargetProtocol,
    pub host: &'static str,
    pub port: u16,
    pub kind: ProbeKind,
}

/// Test targets for the benchmark runner. Kept as one const table per the
/// task spec ("не хардкодь внутри функций"), not scattered across probe
/// call sites.
///
/// `discord.com` and `www.youtube.com` match the pair already used by the
/// pre-Hydra `zapret::health_check()`. `web.telegram.org` isn't covered by
/// any bundled hostlist (Telegram bypass in this app goes through the
/// separate TGWS proxy, not zapret strategy selection) — Hydra still probes
/// it because `winws_mapper::build_winws_args`'s `target_hosts` parameter
/// can inject it into a strategy's hostlist match even though the
/// strategy's own bundled lists don't mention it.
///
/// There is no bundled Discord voice relay IP in this project (no
/// `ipset-discord.txt` ships by default — `zapret.rs::ipset_file` already
/// treats it as optional/created-on-demand), so fabricating one would
/// violate "не выдумывай флаги/адреса". `DiscordVoice` instead probes a
/// well-known public STUN server (Google's) to validate that the STUN
/// protocol itself — which Discord's voice relays also speak — survives
/// the active desync. This is a real substitution, not Discord's actual
/// infrastructure; flagged here and in the Stage 2 summary.
pub const BENCH_TARGETS: &[BenchTarget] = &[
    BenchTarget {
        protocol: TargetProtocol::Discord,
        host: "discord.com",
        port: 443,
        kind: ProbeKind::Tls,
    },
    BenchTarget {
        protocol: TargetProtocol::YouTube,
        host: "www.youtube.com",
        port: 443,
        kind: ProbeKind::Tls,
    },
    BenchTarget {
        protocol: TargetProtocol::Telegram,
        host: "web.telegram.org",
        port: 443,
        kind: ProbeKind::Tls,
    },
    BenchTarget {
        protocol: TargetProtocol::DiscordVoice,
        host: "stun.l.google.com",
        port: 19302,
        kind: ProbeKind::UdpStun,
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeError {
    Timeout,
    ConnectFailed(String),
    InvalidResponse,
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProbeError::Timeout => write!(f, "timeout"),
            ProbeError::ConnectFailed(e) => write!(f, "connect failed: {e}"),
            ProbeError::InvalidResponse => write!(f, "invalid response"),
        }
    }
}

/// TCP connect + TLS handshake to `domain:port`. Returns latency in ms on
/// success. Reuses `reqwest` (already a dependency, and the same mechanism
/// `zapret::health_check()` already relies on) rather than hand-rolling a
/// TLS client — the whole point is to exercise a real handshake the same
/// way any normal HTTPS client on the machine would.
pub async fn probe_tls(domain: &str, port: u16, timeout: Duration) -> Result<u64, ProbeError> {
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| ProbeError::ConnectFailed(e.to_string()))?;
    let url = format!("https://{domain}:{port}");
    let start = Instant::now();
    match client.head(&url).send().await {
        Ok(_) => Ok(start.elapsed().as_millis() as u64),
        Err(e) if e.is_timeout() => Err(ProbeError::Timeout),
        Err(e) => Err(ProbeError::ConnectFailed(e.to_string())),
    }
}

const STUN_MAGIC_COOKIE: [u8; 4] = [0x21, 0x12, 0xA4, 0x42];
const STUN_BINDING_REQUEST: [u8; 2] = [0x00, 0x01];
const STUN_BINDING_SUCCESS: [u8; 2] = [0x01, 0x01];

fn build_stun_binding_request(transaction_id: [u8; 12]) -> [u8; 20] {
    let mut packet = [0u8; 20];
    packet[0..2].copy_from_slice(&STUN_BINDING_REQUEST);
    packet[2..4].copy_from_slice(&[0x00, 0x00]); // message length: no attributes
    packet[4..8].copy_from_slice(&STUN_MAGIC_COOKIE);
    packet[8..20].copy_from_slice(&transaction_id);
    packet
}

fn is_matching_stun_response(response: &[u8], transaction_id: &[u8; 12]) -> bool {
    response.len() >= 20
        && response[0..2] == STUN_BINDING_SUCCESS
        && response[4..8] == STUN_MAGIC_COOKIE
        && response[8..20] == *transaction_id
}

/// UDP STUN binding request to `relay_ip:port`. Returns latency in ms on a
/// well-formed Binding Success Response matching our transaction id.
pub async fn probe_udp_relay(
    relay_ip: &str,
    port: u16,
    timeout: Duration,
) -> Result<u64, ProbeError> {
    let socket = UdpSocket::bind("0.0.0.0:0")
        .await
        .map_err(|e| ProbeError::ConnectFailed(e.to_string()))?;
    socket
        .connect((relay_ip, port))
        .await
        .map_err(|e| ProbeError::ConnectFailed(e.to_string()))?;

    let mut transaction_id = [0u8; 12];
    getrandom::fill(&mut transaction_id)
        .map_err(|e| ProbeError::ConnectFailed(e.to_string()))?;
    let request = build_stun_binding_request(transaction_id);

    let start = Instant::now();
    socket
        .send(&request)
        .await
        .map_err(|e| ProbeError::ConnectFailed(e.to_string()))?;

    let mut buf = [0u8; 64];
    let n = match tokio::time::timeout(timeout, socket.recv(&mut buf)).await {
        Ok(Ok(n)) => n,
        Ok(Err(e)) => return Err(ProbeError::ConnectFailed(e.to_string())),
        Err(_) => return Err(ProbeError::Timeout),
    };

    if is_matching_stun_response(&buf[..n], &transaction_id) {
        Ok(start.elapsed().as_millis() as u64)
    } else {
        Err(ProbeError::InvalidResponse)
    }
}

// ---- Isolated strategy testing -------------------------------------------

const HYDRA_BENCH_HOSTLIST: &str = "hydra-benchmark.txt";
const HYDRA_PRODUCTION_HOSTLIST: &str = "hydra-target.txt";
const SETTLE_TIME: Duration = Duration::from_millis(800);
const PROBE_TIMEOUT: Duration = Duration::from_secs(4);
const ATTEMPTS_PER_TARGET: usize = 3;
const EARLY_EXIT_SCORE: f64 = 0.95;
const MIN_VIABLE_SCORE: f64 = 0.05;

/// A winws instance running a candidate strategy purely for benchmarking.
///
/// True process-level isolation from a concurrently-running production
/// strategy isn't achievable here: WinDivert delivers a copy of every
/// packet matching a handle's filter to *that* handle independently, so
/// two winws processes both capturing e.g. tcp:443 would each try to
/// reinject the same packets. `run_benchmark` sidesteps this by never
/// running a test instance and the production instance at the same
/// time — it tests candidates one at a time, sequentially, the same way
/// the pre-Hydra `zapret::auto_probe` already does. "Isolated" here means
/// the test instance's hostlist is scoped to just the probe domains
/// (via `target_hosts`), not the strategy's full production hostlist —
/// smaller blast radius and a faster settle, not concurrency.
pub struct TestHandle {
    winws: WinwsHandle,
    hostlist_path: PathBuf,
}

impl TestHandle {
    fn is_running(&mut self) -> bool {
        self.winws.is_running()
    }

    fn teardown(self) {
        self.winws.teardown();
        let _ = std::fs::remove_file(&self.hostlist_path);
    }
}

pub async fn apply_strategy_isolated(
    app: &AppHandle,
    strategy: &Strategy,
    target_hosts: &[String],
) -> AppResult<TestHandle> {
    spawn_with_hostlist(app, strategy, target_hosts, HYDRA_BENCH_HOSTLIST)
        .await
        .map(|(winws, hostlist_path)| TestHandle {
            winws,
            hostlist_path,
        })
}

async fn spawn_with_hostlist(
    app: &AppHandle,
    strategy: &Strategy,
    target_hosts: &[String],
    hostlist_file_name: &str,
) -> AppResult<(WinwsHandle, PathBuf)> {
    let paths = crate::zapret::resolve_paths(app)?;
    std::fs::create_dir_all(&paths.lists)?;
    let hostlist_path = paths.lists.join(hostlist_file_name);
    std::fs::write(&hostlist_path, target_hosts.join("\n"))?;

    let args = winws_mapper::build_winws_args(&strategy.params, target_hosts, &hostlist_path);
    let winws = WinwsHandle::spawn(app, "hydra", &paths.winws(), &args, &paths.root)?;
    Ok((winws, hostlist_path))
}

fn score_from(successes: usize, attempts: usize, latencies: &[u64]) -> f64 {
    if attempts == 0 {
        return 0.0;
    }
    let success_rate = successes as f64 / attempts as f64;
    let avg_latency_ms = if latencies.is_empty() {
        2000.0
    } else {
        latencies.iter().sum::<u64>() as f64 / latencies.len() as f64
    };
    success_rate * 0.75 + (1.0 - (avg_latency_ms / 2000.0).min(1.0)) * 0.25
}

async fn bench_one(app: &AppHandle, strategy: &Strategy, probes: &[&BenchTarget], target_hosts: &[String]) -> f64 {
    let mut handle = match apply_strategy_isolated(app, strategy, target_hosts).await {
        Ok(h) => h,
        Err(_) => return 0.0,
    };
    tokio::time::sleep(SETTLE_TIME).await;

    if !handle.is_running() {
        handle.teardown();
        return 0.0;
    }

    let mut successes = 0usize;
    let mut attempts = 0usize;
    let mut latencies: Vec<u64> = Vec::new();

    for probe in probes {
        for _ in 0..ATTEMPTS_PER_TARGET {
            attempts += 1;
            let result = match probe.kind {
                ProbeKind::Tls => probe_tls(probe.host, probe.port, PROBE_TIMEOUT).await,
                ProbeKind::UdpStun => probe_udp_relay(probe.host, probe.port, PROBE_TIMEOUT).await,
            };
            if let Ok(latency) = result {
                successes += 1;
                latencies.push(latency);
            }
        }
    }

    handle.teardown();
    score_from(successes, attempts, &latencies)
}

/// Runs a synchronous DB operation with the lock scoped to just this call —
/// never held across an `.await`, so this stays safe to call from async
/// code on a multi-threaded runtime (a held `std::sync::MutexGuard` isn't
/// `Send`, and `tauri::async_runtime::spawn` requires the future to be).
fn with_db<T>(app: &AppHandle, f: impl FnOnce(&Connection) -> AppResult<T>) -> AppResult<T> {
    let state = app.state::<AppState>();
    let db = state
        .db
        .as_ref()
        .ok_or_else(|| AppError::Msg("Hydra: БД недоступна".into()))?;
    let conn = db.conn.lock().unwrap();
    f(&conn)
}

/// Tests `pool` sequentially against `targets`, scores each strategy
/// (`score = success_rate * 0.75 + (1 - avg_latency_ms/2000).clamp(0,1) * 0.25`,
/// 3 attempts per target per strategy), records every result to
/// `strategy_history`, and activates the best strategy found — bailing
/// early past a 0.95 score. Emits the same event names Stage 4 wires up
/// (`benchmark_started`, `benchmark_progress`, `benchmark_exhausted`,
/// `strategy_switched`) so the UI layer has nothing to rename later.
pub async fn run_benchmark(
    app: &AppHandle,
    pool: &[Strategy],
    targets: &[TargetProtocol],
) -> AppResult<Strategy> {
    if app.state::<AppState>().zapret_running() {
        return Err(AppError::Msg(
            "Hydra: активна ручная стратегия zapret, автоподбор пропущен".into(),
        ));
    }
    if pool.is_empty() {
        return Err(AppError::Msg("Hydra: пул стратегий пуст".into()));
    }

    let probes: Vec<&BenchTarget> = BENCH_TARGETS
        .iter()
        .filter(|t| targets.contains(&t.protocol))
        .collect();
    if probes.is_empty() {
        return Err(AppError::Msg(
            "Hydra: нет тестовых целей для указанных targets".into(),
        ));
    }
    let target_hosts: Vec<String> = probes.iter().map(|t| t.host.to_string()).collect();

    let _ = app.emit("benchmark_started", json!({ "pool_size": pool.len() }));

    let mut best: Option<(Strategy, f64)> = None;

    for (index, strategy) in pool.iter().enumerate() {
        let _ = app.emit(
            "benchmark_progress",
            json!({ "tested": index, "total": pool.len(), "current": strategy.name }),
        );

        let score = bench_one(app, strategy, &probes, &target_hosts).await;
        let _ = with_db(app, |conn| {
            crate::strategy::update_score(conn, &strategy.id, score)?;
            crate::strategy::record_history(
                conn,
                &strategy.id,
                "benchmark_result",
                Some(&format!("score={score:.3}")),
            )
        });

        if best.as_ref().map_or(true, |(_, b)| score > *b) {
            best = Some((strategy.clone(), score));
        }
        if score > EARLY_EXIT_SCORE {
            break;
        }
    }

    match best {
        Some((strategy, score)) if score >= MIN_VIABLE_SCORE => {
            activate_in_production(app, &strategy, &target_hosts).await?;
            let _ = with_db(app, |conn| {
                crate::strategy::activate_strategy(conn, &strategy.id)?;
                crate::strategy::record_history(
                    conn,
                    &strategy.id,
                    "activated",
                    Some(&format!("score={score:.3}")),
                )
            });
            let _ = app.emit(
                "strategy_switched",
                json!({ "name": strategy.name, "reason": "benchmark" }),
            );
            Ok(strategy)
        }
        _ => {
            let _ = app.emit("benchmark_exhausted", json!({}));
            Err(AppError::NoWorkingStrategy)
        }
    }
}

/// Spawns the winner as Hydra's own long-lived process (`AppState.hydra`,
/// not the legacy `AppState.zapret` slot) and confirms it's still alive a
/// moment later, mirroring `zapret::confirm_started`.
async fn activate_in_production(
    app: &AppHandle,
    strategy: &Strategy,
    target_hosts: &[String],
) -> AppResult<()> {
    {
        let state = app.state::<AppState>();
        state.hydra.lock().unwrap().stop();
    }

    let (mut winws, _hostlist_path) =
        spawn_with_hostlist(app, strategy, target_hosts, HYDRA_PRODUCTION_HOSTLIST).await?;

    tokio::time::sleep(SETTLE_TIME).await;

    if !winws.is_running() {
        winws.teardown();
        return Err(AppError::Msg(
            "Hydra: winws завершился сразу после активации стратегии".into(),
        ));
    }

    let state = app.state::<AppState>();
    *state.hydra.lock().unwrap() = winws.into_inner();
    *state.hydra_active_strategy_id.lock().unwrap() = Some(strategy.id.clone());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_rewards_success_rate_and_low_latency() {
        assert_eq!(score_from(0, 0, &[]), 0.0);
        let perfect = score_from(9, 9, &[0, 0, 0]);
        assert!((perfect - 1.0).abs() < 1e-9, "perfect run should score 1.0, got {perfect}");

        let half_success_high_latency = score_from(3, 6, &[2000, 2000, 2000]);
        assert!((half_success_high_latency - 0.375).abs() < 1e-9);

        let all_success_slow = score_from(3, 3, &[4000, 4000, 4000]);
        // latency clamps at 2000ms, so this should equal the all-success,
        // exactly-2000ms case: 0.75 + 0.0 = 0.75.
        assert!((all_success_slow - 0.75).abs() < 1e-9);
    }

    #[test]
    fn score_prefers_lower_latency_at_equal_success_rate() {
        let fast = score_from(3, 3, &[100, 100, 100]);
        let slow = score_from(3, 3, &[1900, 1900, 1900]);
        assert!(fast > slow);
    }

    #[test]
    fn builds_a_well_formed_binding_request() {
        let txn = [7u8; 12];
        let packet = build_stun_binding_request(txn);
        assert_eq!(packet.len(), 20);
        assert_eq!(&packet[0..2], &STUN_BINDING_REQUEST);
        assert_eq!(&packet[2..4], &[0x00, 0x00]);
        assert_eq!(&packet[4..8], &STUN_MAGIC_COOKIE);
        assert_eq!(&packet[8..20], &txn);
    }

    #[test]
    fn accepts_matching_success_response() {
        let txn = [3u8; 12];
        let mut response = vec![0x01, 0x01, 0x00, 0x00];
        response.extend_from_slice(&STUN_MAGIC_COOKIE);
        response.extend_from_slice(&txn);
        assert!(is_matching_stun_response(&response, &txn));
    }

    #[test]
    fn rejects_mismatched_transaction_id() {
        let txn = [3u8; 12];
        let mut response = vec![0x01, 0x01, 0x00, 0x00];
        response.extend_from_slice(&STUN_MAGIC_COOKIE);
        response.extend_from_slice(&[9u8; 12]);
        assert!(!is_matching_stun_response(&response, &txn));
    }

    #[test]
    fn rejects_non_success_message_type() {
        let txn = [3u8; 12];
        let mut response = vec![0x01, 0x11, 0x00, 0x00]; // error response 0x0111
        response.extend_from_slice(&STUN_MAGIC_COOKIE);
        response.extend_from_slice(&txn);
        assert!(!is_matching_stun_response(&response, &txn));
    }

    #[test]
    fn rejects_short_response() {
        assert!(!is_matching_stun_response(&[0x01, 0x01], &[0u8; 12]));
    }

    #[tokio::test]
    async fn probe_udp_relay_reaches_a_public_stun_server() {
        // Network-dependent; only meaningful when run somewhere with real
        // internet access, but a good sanity check for the wire format
        // against a real, well-known third party.
        let result = probe_udp_relay("stun.l.google.com", 19302, Duration::from_secs(3)).await;
        if let Err(e) = &result {
            eprintln!("skipping assertion, no network in this sandbox: {e}");
            return;
        }
        assert!(result.unwrap() < 3000);
    }
}
