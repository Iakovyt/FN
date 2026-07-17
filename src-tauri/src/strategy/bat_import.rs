//! One-off importer: turns a Flowseal-style `general*.bat` strategy (see
//! `resources/zapret/general*.bat`) into a [`BypassParams`]. Used to
//! generate the committed `assets/builtin_strategies.json` snapshot — see
//! the `#[ignore]`d generator test at the bottom of this file, rerun it
//! manually (`cargo test -p fn-app --lib generate_builtin_strategies_json
//! -- --ignored --nocapture`) whenever the bundled zapret `.bat` files are
//! upgraded.
//!
//! Every flag handled here is one actually observed across all 20 bundled
//! `general*.bat` files (checked by hand). Anything unrecognized is a hard
//! error rather than a silent drop, so a future zapret update that adds new
//! flags fails the importer loudly instead of quietly losing data.

use std::path::Path;

use crate::strategies::{extract_winws_command, split_command_line};
use crate::strategy::schema::{
    BypassParams, DesyncLeg, DesyncMode, FakeTlsCfg, LegProto, SplitMarker,
};

/// Placeholder values zapret's own "auto fake TLS" idiom passes to
/// `--dpi-desync-fake-tls` when paired with `--dpi-desync-fake-tls-mod=rnd,...`
/// — not real ClientHello content, so we drop them and let the mapper
/// re-supply a canonical placeholder whenever `fake_tls_mod` is set.
const FAKE_TLS_AUTO_PLACEHOLDERS: &[&str] = &["0x00000000", "^!", "!"];

pub(crate) fn parse_bat(raw_text: &str, bin_dir: &Path) -> Result<BypassParams, String> {
    let command =
        extract_winws_command(raw_text).ok_or("no winws.exe invocation found in file")?;
    let tokens = split_command_line(&command);

    let mut legs = Vec::new();
    let mut current: Option<DesyncLeg> = None;
    // `--filter-l3` (when present) precedes the `--filter-tcp/udp` that
    // opens its leg, so it can't be applied to `current` directly.
    let mut pending_filter_l3: Option<String> = None;

    for token in tokens {
        if token == "--new" {
            if let Some(leg) = current.take() {
                legs.push(leg);
            }
            continue;
        }
        if token.starts_with("--wf-tcp=") || token.starts_with("--wf-udp=") {
            // Recomputed from the legs themselves in winws_mapper; the
            // top-level capture filter carries no per-leg information.
            continue;
        }

        let (flag, value) = token
            .split_once('=')
            .map(|(f, v)| (f, Some(v)))
            .unwrap_or((token.as_str(), None));

        match flag {
            "--filter-l3" => {
                pending_filter_l3 = Some(require(value, flag)?.to_string());
            }
            "--filter-tcp" => {
                finish_leg(&mut legs, &mut current);
                let mut leg = DesyncLeg::new(LegProto::Tcp, require(value, flag)?);
                leg.filter_l3 = pending_filter_l3.take();
                current = Some(leg);
            }
            "--filter-udp" => {
                finish_leg(&mut legs, &mut current);
                let mut leg = DesyncLeg::new(LegProto::Udp, require(value, flag)?);
                leg.filter_l3 = pending_filter_l3.take();
                current = Some(leg);
            }
            _ => {
                let leg = current
                    .as_mut()
                    .ok_or_else(|| format!("flag {flag} before any --filter-tcp/udp"))?;
                apply_flag(leg, flag, value, bin_dir)?;
            }
        }
    }
    finish_leg(&mut legs, &mut current);

    if legs.is_empty() {
        return Err("no legs parsed".into());
    }
    Ok(BypassParams { legs })
}

fn finish_leg(legs: &mut Vec<DesyncLeg>, current: &mut Option<DesyncLeg>) {
    if let Some(leg) = current.take() {
        legs.push(leg);
    }
}

fn require<'a>(value: Option<&'a str>, flag: &str) -> Result<&'a str, String> {
    value.ok_or_else(|| format!("{flag} missing a value"))
}

fn apply_flag(
    leg: &mut DesyncLeg,
    flag: &str,
    value: Option<&str>,
    bin_dir: &Path,
) -> Result<(), String> {
    match flag {
        "--filter-l7" => {
            leg.filter_l7 = split_csv(require(value, flag)?);
        }
        "--hostlist" => {
            leg.hosts
                .hostlist_files
                .push(strip_lists_marker(require(value, flag)?));
        }
        "--hostlist-exclude" => {
            leg.hosts
                .hostlist_exclude_files
                .push(strip_lists_marker(require(value, flag)?));
        }
        "--hostlist-domains" => {
            leg.hosts.hostlist_domains = split_csv(require(value, flag)?);
        }
        "--ipset" => {
            leg.hosts
                .ipset_files
                .push(strip_lists_marker(require(value, flag)?));
        }
        "--ipset-exclude" => {
            leg.hosts
                .ipset_exclude_files
                .push(strip_lists_marker(require(value, flag)?));
        }
        "--ip-id" => {
            if require(value, flag)? == "zero" {
                leg.ip_id_zero = true;
            } else {
                return Err(format!("unsupported --ip-id value: {value:?}"));
            }
        }
        "--dpi-desync" => {
            for mode in split_csv(require(value, flag)?) {
                leg.desync_mode.push(parse_desync_mode(&mode)?);
            }
        }
        "--dpi-desync-repeats" => {
            leg.repeats = require(value, flag)?
                .parse()
                .map_err(|e| format!("bad repeats value: {e}"))?;
        }
        "--dpi-desync-ttl" => {
            leg.ttl = Some(
                require(value, flag)?
                    .parse()
                    .map_err(|e| format!("bad ttl value: {e}"))?,
            );
        }
        "--dpi-desync-autottl" => {
            leg.autottl = true;
        }
        "--dpi-desync-split-pos" => {
            for marker in split_csv(require(value, flag)?) {
                leg.split_position.push(parse_split_marker(&marker)?);
            }
        }
        "--dpi-desync-split-seqovl" => {
            leg.split_seqovl = Some(
                require(value, flag)?
                    .parse()
                    .map_err(|e| format!("bad split-seqovl value: {e}"))?,
            );
        }
        "--dpi-desync-split-seqovl-pattern" => {
            leg.split_seqovl_pattern_hex = Some(read_bin_hex(require(value, flag)?, bin_dir)?);
        }
        "--dpi-desync-fakedsplit-pattern" => {
            leg.fakedsplit_pattern_hex = Some(parse_inline_hex(require(value, flag)?)?);
        }
        "--dpi-desync-fooling" => {
            leg.fooling = split_csv(require(value, flag)?);
        }
        "--dpi-desync-badseq-increment" => {
            leg.badseq_increment = Some(
                require(value, flag)?
                    .parse()
                    .map_err(|e| format!("bad badseq-increment value: {e}"))?,
            );
        }
        "--dpi-desync-fake-tls" => {
            let raw = require(value, flag)?;
            if FAKE_TLS_AUTO_PLACEHOLDERS.contains(&raw) {
                // handled by --dpi-desync-fake-tls-mod instead
            } else {
                leg.fake_tls.push(FakeTlsCfg {
                    sni_domain: guess_sni_domain(raw),
                    fake_data_hex: Some(read_bin_hex(raw, bin_dir)?),
                });
            }
        }
        "--dpi-desync-fake-tls-mod" => {
            leg.fake_tls_mod = Some(require(value, flag)?.to_string());
        }
        "--dpi-desync-fake-quic" => {
            leg.fake_quic_hex.push(read_bin_hex(require(value, flag)?, bin_dir)?);
        }
        "--dpi-desync-fake-discord" => {
            leg.fake_discord_hex
                .push(read_bin_hex(require(value, flag)?, bin_dir)?);
        }
        "--dpi-desync-fake-stun" => {
            leg.fake_stun_hex.push(read_bin_hex(require(value, flag)?, bin_dir)?);
        }
        "--dpi-desync-fake-http" => {
            leg.fake_http_hex.push(read_bin_hex(require(value, flag)?, bin_dir)?);
        }
        "--dpi-desync-fake-unknown-udp" => {
            leg.fake_unknown_udp_hex
                .push(read_bin_hex(require(value, flag)?, bin_dir)?);
        }
        "--dpi-desync-hostfakesplit-mod" => {
            let raw = require(value, flag)?;
            let host = raw
                .strip_prefix("host=")
                .ok_or_else(|| format!("unsupported hostfakesplit-mod: {raw}"))?;
            leg.hostfakesplit_host = Some(host.to_string());
        }
        "--dpi-desync-any-protocol" => {
            leg.any_protocol = true;
        }
        "--dpi-desync-cutoff" => {
            leg.cutoff = Some(require(value, flag)?.to_string());
        }
        other => return Err(format!("unrecognized flag: {other}")),
    }
    Ok(())
}

fn parse_split_marker(token: &str) -> Result<SplitMarker, String> {
    match token {
        "sni" => Ok(SplitMarker::Sni),
        "midsld" => Ok(SplitMarker::Midsld),
        "sniext" => Ok(SplitMarker::SniExt(0)),
        rest if rest.starts_with("sniext") => rest["sniext".len()..]
            .parse::<i32>()
            .map(SplitMarker::SniExt)
            .map_err(|e| format!("bad sniext offset in {token}: {e}")),
        n => n
            .parse()
            .map(SplitMarker::Fixed)
            .map_err(|e| format!("bad split-pos marker {n}: {e}")),
    }
}

fn parse_desync_mode(token: &str) -> Result<DesyncMode, String> {
    match token {
        "fake" => Ok(DesyncMode::Fake),
        "multisplit" => Ok(DesyncMode::Multisplit),
        "multidisorder" => Ok(DesyncMode::Multidisorder),
        "fakedsplit" => Ok(DesyncMode::FakeDSplit),
        "hostfakesplit" => Ok(DesyncMode::HostFakeSplit),
        "syndata" => Ok(DesyncMode::SynData),
        other => Err(format!("unrecognized dpi-desync mode: {other}")),
    }
}

fn parse_inline_hex(value: &str) -> Result<String, String> {
    value
        .strip_prefix("0x")
        .map(|hex| hex.to_string())
        .ok_or_else(|| format!("expected an inline 0x-hex value, got: {value}"))
}

fn split_csv(value: &str) -> Vec<String> {
    value.split(',').map(|s| s.trim().to_string()).collect()
}

fn strip_lists_marker(value: &str) -> String {
    value.strip_prefix("%LISTS%").unwrap_or(value).to_string()
}

fn read_bin_hex(value: &str, bin_dir: &Path) -> Result<String, String> {
    let filename = value
        .strip_prefix("%BIN%")
        .ok_or_else(|| format!("expected a %BIN%-relative file, got: {value}"))?;
    let path = bin_dir.join(filename);
    let bytes = std::fs::read(&path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    Ok(crate::strategy::schema::encode_hex(&bytes))
}

/// Best-effort label from a `tls_clienthello_<slug>.bin`-style filename
/// (e.g. `tls_clienthello_www_google_com.bin` -> `www.google.com`); purely
/// cosmetic metadata, not used by `winws_mapper` for file-backed blobs.
fn guess_sni_domain(value: &str) -> String {
    let filename = value.strip_prefix("%BIN%").unwrap_or(value);
    let stem = filename.strip_suffix(".bin").unwrap_or(filename);
    match stem.strip_prefix("tls_clienthello_") {
        Some(slug) => slug.replace('_', "."),
        None => "www.google.com".to_string(),
    }
}

#[cfg(test)]
mod generator {
    use super::*;
    use crate::strategy::schema::{Strategy, StrategySource, TargetProtocol};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Regenerates `assets/builtin_strategies.json` from the bundled
    /// `resources/zapret/general*.bat` files. Not run as part of the normal
    /// suite (`#[ignore]`) — it performs real file I/O against the repo and
    /// is meant to be rerun by hand when those `.bat` files change:
    /// `cargo test --lib bat_import::generator::generate_builtin_strategies_json -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn generate_builtin_strategies_json() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let zapret_root = manifest_dir.join("resources").join("zapret");
        let bin_dir = zapret_root.join("bin");
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let mut names: Vec<String> = std::fs::read_dir(&zapret_root)
            .expect("read resources/zapret")
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let path = entry.path();
                let name = path.file_name()?.to_str()?.to_string();
                let is_general_bat = name.to_ascii_lowercase().starts_with("general")
                    && name.to_ascii_lowercase().ends_with(".bat");
                is_general_bat.then_some(name)
            })
            .collect();
        names.sort();
        assert!(!names.is_empty(), "no general*.bat files found");

        let mut strategies = Vec::new();
        for file_name in names {
            let path = zapret_root.join(&file_name);
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
            let params = parse_bat(&text, &bin_dir)
                .unwrap_or_else(|e| panic!("parsing {}: {e}", path.display()));
            let display_name = file_name
                .strip_suffix(".bat")
                .unwrap_or(&file_name)
                .to_string();
            strategies.push(Strategy::new(
                display_name,
                TargetProtocol::Generic("general".into()),
                params,
                StrategySource::Builtin,
                created_at,
            ));
        }

        let json = serde_json::to_string_pretty(&strategies).expect("serialize strategies");
        let out_path = manifest_dir
            .join("assets")
            .join("builtin_strategies.json");
        std::fs::create_dir_all(out_path.parent().unwrap()).expect("create assets dir");
        std::fs::write(&out_path, json).expect("write builtin_strategies.json");
        println!(
            "wrote {} strategies to {}",
            strategies.len(),
            out_path.display()
        );
    }
}
