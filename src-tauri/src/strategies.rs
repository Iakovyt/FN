use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{AppError, AppResult};

/// Metadata sent to the frontend dropdown.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StrategyInfo {
    pub id: String,
    pub name: String,
    pub gaming: bool,
    pub auto: bool,
}

/// Resolved on-disk locations inside the extracted zapret folder.
#[derive(Debug, Clone)]
pub struct ZapretPaths {
    pub root: PathBuf,
    pub bin: PathBuf,
    pub lists: PathBuf,
}

impl ZapretPaths {
    pub fn winws(&self) -> PathBuf {
        self.bin.join("winws.exe")
    }

    /// A file under `lists/`, or `None` when it does not exist on disk.
    fn list_file(&self, name: &str) -> Option<PathBuf> {
        let p = self.lists.join(name);
        exists(&p)
    }

    /// A fake-payload blob under `bin/`, or `None` when missing.
    fn bin_file(&self, name: &str) -> Option<PathBuf> {
        let p = self.bin.join(name);
        exists(&p)
    }
}

fn exists(p: &Path) -> Option<PathBuf> {
    if p.exists() {
        Some(p.to_path_buf())
    } else {
        None
    }
}

pub const AUTO_ID: &str = "auto";
pub const BATCH_PREFIX: &str = "bat:";

/// The concrete strategies the auto-probe walks through, in order.
pub const PROBE_ORDER: &[&str] = &[
    "general_alt",
    "general_fake_tls",
    "multisplit",
    "multidisorder",
];

fn auto_strategy() -> StrategyInfo {
    StrategyInfo {
        id: AUTO_ID.into(),
        name: "Авто-подбор (рекомендуется)".into(),
        gaming: false,
        auto: true,
    }
}

/// Read launchable BAT strategies from the active Zapret folder.
pub fn list_for_root(root: &Path) -> AppResult<Vec<StrategyInfo>> {
    let mut batches = Vec::new();
    if root.is_dir() {
        for entry in std::fs::read_dir(root)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file()
                || !path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("bat"))
            {
                continue;
            }
            let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if file_name.eq_ignore_ascii_case("service.bat") || !batch_has_winws(&path) {
                continue;
            }
            batches.push(StrategyInfo {
                id: format!("{BATCH_PREFIX}{file_name}"),
                name: path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or(file_name)
                    .to_string(),
                gaming: false,
                auto: false,
            });
        }
    }

    batches.sort_by_key(|strategy| strategy.name.to_ascii_lowercase());
    let mut out = vec![auto_strategy()];
    if batches.is_empty() {
        out.extend(fallback_list());
    } else {
        out.extend(batches);
    }
    Ok(out)
}

fn fallback_list() -> Vec<StrategyInfo> {
    let mut out = Vec::new();
    for id in PROBE_ORDER {
        out.push(StrategyInfo {
            id: (*id).into(),
            name: display_name(id).into(),
            gaming: false,
            auto: false,
        });
    }
    out
}

fn display_name(id: &str) -> &'static str {
    match id {
        "general_alt" => "general (ALT)",
        "general_fake_tls" => "general (FAKE TLS)",
        "multisplit" => "multisplit",
        "multidisorder" => "multidisorder",
        _ => "general",
    }
}

pub fn is_known(id: &str) -> bool {
    id == AUTO_ID || PROBE_ORDER.contains(&id)
}

pub fn is_known_for_root(id: &str, root: &Path) -> bool {
    is_known(id) || batch_file(root, id).is_some_and(|path| batch_has_winws(&path))
}

pub fn is_batch(id: &str) -> bool {
    id.starts_with(BATCH_PREFIX)
}

fn batch_file(root: &Path, id: &str) -> Option<PathBuf> {
    let file_name = id.strip_prefix(BATCH_PREFIX)?;
    let safe_name = Path::new(file_name);
    if file_name.is_empty() || safe_name.file_name()?.to_str()? != file_name {
        return None;
    }
    let path = root.join(safe_name);
    path.is_file().then_some(path)
}

fn batch_has_winws(path: &Path) -> bool {
    std::fs::read(path)
        .ok()
        .map(|bytes| {
            String::from_utf8_lossy(&bytes)
                .to_ascii_lowercase()
                .contains("winws.exe")
        })
        .unwrap_or(false)
}

/// Extract the continued winws command from a Flowseal-style strategy BAT.
/// winws is then spawned directly, keeping process ownership inside the app.
pub fn build_batch_args(id: &str, paths: &ZapretPaths) -> AppResult<Vec<String>> {
    let path = batch_file(&paths.root, id)
        .filter(|path| batch_has_winws(path))
        .ok_or_else(|| AppError::Msg(format!("Стратегия не найдена: {id}")))?;
    let bytes = std::fs::read(&path)?;
    let text = String::from_utf8_lossy(&bytes);
    let mut command = String::new();
    let mut collecting = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if !collecting {
            let lower = trimmed.to_ascii_lowercase();
            let Some(index) = lower.find("winws.exe") else {
                continue;
            };
            let mut rest = &trimmed[index + "winws.exe".len()..];
            if let Some(without_quote) = rest.strip_prefix('"') {
                rest = without_quote;
            }
            append_command_part(&mut command, rest);
            collecting = trimmed.ends_with('^');
        } else {
            append_command_part(&mut command, trimmed);
            collecting = trimmed.ends_with('^');
        }
        if !collecting {
            break;
        }
    }

    if command.trim().is_empty() {
        return Err(AppError::Msg(format!(
            "В файле {} не найдена команда запуска winws.exe",
            path.display()
        )));
    }

    let bin = with_trailing_separator(&paths.bin);
    let lists = with_trailing_separator(&paths.lists);
    let (game_tcp, game_udp) = game_filter_ports(&paths.root);
    let expanded = command
        .replace("%BIN%", &bin)
        .replace("%LISTS%", &lists)
        .replace("%GameFilterTCP%", game_tcp)
        .replace("%GameFilterUDP%", game_udp);
    let args = split_command_line(&expanded);
    if args.is_empty() {
        return Err(AppError::Msg(format!(
            "Пустая стратегия: {}",
            path.display()
        )));
    }
    Ok(args)
}

fn append_command_part(command: &mut String, part: &str) {
    let part = part.trim_end().strip_suffix('^').unwrap_or(part).trim();
    if !part.is_empty() {
        if !command.is_empty() {
            command.push(' ');
        }
        command.push_str(part);
    }
}

fn with_trailing_separator(path: &Path) -> String {
    let mut value = path.to_string_lossy().into_owned();
    if !value.ends_with(['\\', '/']) {
        value.push('\\');
    }
    value
}

fn game_filter_ports(root: &Path) -> (&'static str, &'static str) {
    let mode =
        std::fs::read_to_string(root.join("utils").join("game_filter.enabled")).unwrap_or_default();
    match mode
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "all" => ("1024-65535", "1024-65535"),
        "tcp" => ("1024-65535", "12"),
        "udp" => ("12", "1024-65535"),
        _ => ("12", "12"),
    }
}

fn split_command_line(command: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for ch in command.chars() {
        match ch {
            '"' => quoted = !quoted,
            ch if ch.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

/// Build the winws argument vector for a concrete strategy id.
///
/// When `gaming` is true we bias towards low latency: fewer fake packets and
/// smaller repeat counts, which trades a little robustness for less overhead.
pub fn build_args(id: &str, paths: &ZapretPaths, gaming: bool) -> Vec<String> {
    let repeats = if gaming { "2" } else { "6" };
    let mut a: Vec<String> = Vec::new();

    // Common capture filter: intercept the relevant TCP/UDP ports.
    push(&mut a, "--wf-tcp=80,443");
    push(&mut a, "--wf-udp=443,50000-65535");

    // Resolve optional data files once.
    let hostlist = paths
        .list_file("list-general.txt")
        .or_else(|| paths.list_file("russia-blacklist.txt"));
    let ipset = paths.list_file("ipset-discord.txt");
    let fake_quic = paths.bin_file("quic_initial_www_google_com.bin");
    let fake_tls = paths.bin_file("tls_clienthello_www_google_com.bin");

    // --- QUIC (UDP/443) leg, shared by all strategies ------------------
    push(&mut a, "--filter-udp=443");
    if let Some(h) = &hostlist {
        push(&mut a, format!("--hostlist={}", h.display()));
    }
    push(&mut a, "--dpi-desync=fake");
    push(&mut a, format!("--dpi-desync-repeats={repeats}"));
    if !gaming {
        if let Some(q) = &fake_quic {
            push(&mut a, format!("--dpi-desync-fake-quic={}", q.display()));
        }
    }
    push(&mut a, "--new");

    // --- Discord voice (UDP high ports) leg ----------------------------
    push(&mut a, "--filter-udp=50000-65535");
    if let Some(ip) = &ipset {
        push(&mut a, format!("--ipset={}", ip.display()));
    }
    push(&mut a, "--dpi-desync=fake");
    push(&mut a, "--dpi-desync-any-protocol");
    push(&mut a, "--dpi-desync-cutoff=d3");
    push(&mut a, format!("--dpi-desync-repeats={repeats}"));
    push(&mut a, "--new");

    // --- TCP/443 leg: this is where strategies actually differ ---------
    push(&mut a, "--filter-tcp=443");
    if let Some(h) = &hostlist {
        push(&mut a, format!("--hostlist={}", h.display()));
    }
    match id {
        "general_alt" => {
            push(&mut a, "--dpi-desync=fake,split2");
            push(&mut a, "--dpi-desync-autottl=2");
            push(&mut a, "--dpi-desync-fooling=md5sig");
        }
        "general_fake_tls" => {
            push(&mut a, "--dpi-desync=fake,split2");
            push(&mut a, format!("--dpi-desync-repeats={repeats}"));
            push(&mut a, "--dpi-desync-fooling=badseq");
            if let Some(t) = &fake_tls {
                push(&mut a, format!("--dpi-desync-fake-tls={}", t.display()));
            }
        }
        "multisplit" => {
            push(&mut a, "--dpi-desync=multisplit");
            push(&mut a, "--dpi-desync-split-pos=1,midsld");
            push(&mut a, "--dpi-desync-fooling=md5sig");
        }
        "multidisorder" => {
            push(&mut a, "--dpi-desync=multidisorder");
            push(&mut a, "--dpi-desync-split-pos=1,midsld");
            push(&mut a, "--dpi-desync-fooling=md5sig");
        }
        _ => {
            // Fallback == general_alt behaviour.
            push(&mut a, "--dpi-desync=fake,split2");
            push(&mut a, "--dpi-desync-autottl=2");
            push(&mut a, "--dpi-desync-fooling=md5sig");
        }
    }

    a
}

fn push(v: &mut Vec<String>, s: impl Into<String>) {
    v.push(s.into());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_batch_strategies_are_discovered_and_parsed() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("zapret");
        let strategies = list_for_root(&root).expect("strategy list");
        assert!(strategies.len() > 10);
        assert!(!strategies.iter().any(|strategy| strategy.name == "service"));

        let selected = strategies
            .iter()
            .find(|strategy| strategy.id == "bat:general (ALT).bat")
            .expect("general ALT strategy");
        let paths = ZapretPaths {
            root: root.clone(),
            bin: root.join("bin"),
            lists: root.join("lists"),
        };
        let args = build_batch_args(&selected.id, &paths).expect("batch arguments");
        assert!(args.iter().any(|arg| arg.starts_with("--wf-tcp=")));
        assert!(args.iter().all(|arg| !arg.contains('%')));
    }
}
