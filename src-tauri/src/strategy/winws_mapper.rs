//! Translates a [`BypassParams`] (a chain of [`DesyncLeg`]s) into the real
//! `winws.exe` command line, following the flags documented at
//! <https://github.com/bol-van/zapret/blob/master/docs/readme.md#nfqws> and
//! exercised by the bundled `resources/zapret/general*.bat` strategies.

use std::path::Path;

use crate::strategy::schema::{BypassParams, DesyncLeg, LegProto};

/// Build the full winws.exe argument vector for `params`.
///
/// `target_hosts` is the current Hydra bypass target's domain list; when
/// non-empty it's referenced via `--hostlist=<hostlist_path>` on every leg,
/// on top of whatever hostlist/ipset files that leg already carries. The
/// caller is responsible for writing `target_hosts` into `hostlist_path` —
/// this function only emits the CLI arguments, it never touches disk.
/// Filenames recorded on each leg's [`HostMatch`] (e.g. `list-general.txt`)
/// are resolved relative to `hostlist_path`'s parent directory, since
/// builtin strategies were imported from files that live next to it under
/// `lists/`.
pub fn build_winws_args(
    params: &BypassParams,
    target_hosts: &[String],
    hostlist_path: &Path,
) -> Vec<String> {
    let lists_dir = hostlist_path.parent().unwrap_or_else(|| Path::new("."));
    let mut args = Vec::new();

    if let Some(wf_tcp) = union_ports(params, LegProto::Tcp) {
        args.push(format!("--wf-tcp={wf_tcp}"));
    }
    if let Some(wf_udp) = union_ports(params, LegProto::Udp) {
        args.push(format!("--wf-udp={wf_udp}"));
    }

    let leg_count = params.legs.len();
    for (index, leg) in params.legs.iter().enumerate() {
        push_leg_args(&mut args, leg, target_hosts, hostlist_path, lists_dir);
        if index + 1 < leg_count {
            args.push("--new".to_string());
        }
    }

    args
}

fn union_ports(params: &BypassParams, proto: LegProto) -> Option<String> {
    let mut seen = Vec::new();
    for leg in &params.legs {
        if leg.proto != proto {
            continue;
        }
        for token in leg.ports.split(',') {
            let token = token.trim();
            if !token.is_empty() && !seen.iter().any(|existing: &String| existing == token) {
                seen.push(token.to_string());
            }
        }
    }
    (!seen.is_empty()).then(|| seen.join(","))
}

fn push_leg_args(
    args: &mut Vec<String>,
    leg: &DesyncLeg,
    target_hosts: &[String],
    hostlist_path: &Path,
    lists_dir: &Path,
) {
    if let Some(l3) = &leg.filter_l3 {
        args.push(format!("--filter-l3={l3}"));
    }
    match leg.proto {
        LegProto::Tcp => args.push(format!("--filter-tcp={}", leg.ports)),
        LegProto::Udp => args.push(format!("--filter-udp={}", leg.ports)),
    }

    if !leg.filter_l7.is_empty() {
        args.push(format!("--filter-l7={}", leg.filter_l7.join(",")));
    }

    for file in &leg.hosts.hostlist_files {
        args.push(format!("--hostlist={}", resolve(lists_dir, file)));
    }
    if !target_hosts.is_empty() {
        args.push(format!("--hostlist={}", hostlist_path.display()));
    }
    if !leg.hosts.hostlist_domains.is_empty() {
        args.push(format!(
            "--hostlist-domains={}",
            leg.hosts.hostlist_domains.join(",")
        ));
    }
    for file in &leg.hosts.hostlist_exclude_files {
        args.push(format!("--hostlist-exclude={}", resolve(lists_dir, file)));
    }
    for file in &leg.hosts.ipset_files {
        args.push(format!("--ipset={}", resolve(lists_dir, file)));
    }
    for file in &leg.hosts.ipset_exclude_files {
        args.push(format!("--ipset-exclude={}", resolve(lists_dir, file)));
    }

    if leg.ip_id_zero {
        args.push("--ip-id=zero".to_string());
    }

    if !leg.desync_mode.is_empty() {
        let modes = leg
            .desync_mode
            .iter()
            .map(|m| m.token())
            .collect::<Vec<_>>()
            .join(",");
        args.push(format!("--dpi-desync={modes}"));
    }

    if leg.repeats > 0 {
        args.push(format!("--dpi-desync-repeats={}", leg.repeats));
    }
    if let Some(ttl) = leg.ttl {
        args.push(format!("--dpi-desync-ttl={ttl}"));
    }
    if leg.autottl {
        args.push("--dpi-desync-autottl".to_string());
    }

    if !leg.split_position.is_empty() {
        let markers = leg
            .split_position
            .iter()
            .map(|m| m.token())
            .collect::<Vec<_>>()
            .join(",");
        args.push(format!("--dpi-desync-split-pos={markers}"));
    }
    if let Some(seqovl) = leg.split_seqovl {
        args.push(format!("--dpi-desync-split-seqovl={seqovl}"));
    }
    if let Some(hex) = &leg.split_seqovl_pattern_hex {
        args.push(format!("--dpi-desync-split-seqovl-pattern=0x{hex}"));
    }
    if let Some(hex) = &leg.fakedsplit_pattern_hex {
        args.push(format!("--dpi-desync-fakedsplit-pattern=0x{hex}"));
    }

    if !leg.fooling.is_empty() {
        args.push(format!("--dpi-desync-fooling={}", leg.fooling.join(",")));
    }
    if let Some(increment) = leg.badseq_increment {
        args.push(format!("--dpi-desync-badseq-increment={increment}"));
    }

    for fake in &leg.fake_tls {
        if let Some(hex) = &fake.fake_data_hex {
            args.push(format!("--dpi-desync-fake-tls=0x{hex}"));
        }
    }
    if let Some(fake_tls_mod) = &leg.fake_tls_mod {
        if leg.fake_tls.is_empty() {
            // winws still requires a base value even when -mod takes over
            // (e.g. `rnd,...` auto-generation); zapret's own idiom for this
            // is a dummy all-zero placeholder, mirrored here.
            args.push("--dpi-desync-fake-tls=0x00000000".to_string());
        }
        args.push(format!("--dpi-desync-fake-tls-mod={fake_tls_mod}"));
    }
    for hex in &leg.fake_quic_hex {
        args.push(format!("--dpi-desync-fake-quic=0x{hex}"));
    }
    for hex in &leg.fake_discord_hex {
        args.push(format!("--dpi-desync-fake-discord=0x{hex}"));
    }
    for hex in &leg.fake_stun_hex {
        args.push(format!("--dpi-desync-fake-stun=0x{hex}"));
    }
    for hex in &leg.fake_http_hex {
        args.push(format!("--dpi-desync-fake-http=0x{hex}"));
    }
    for hex in &leg.fake_unknown_udp_hex {
        args.push(format!("--dpi-desync-fake-unknown-udp=0x{hex}"));
    }
    if let Some(host) = &leg.hostfakesplit_host {
        args.push(format!("--dpi-desync-hostfakesplit-mod=host={host}"));
    }

    if leg.any_protocol {
        args.push("--dpi-desync-any-protocol".to_string());
    }
    if let Some(cutoff) = &leg.cutoff {
        args.push(format!("--dpi-desync-cutoff={cutoff}"));
    }
}

fn resolve(lists_dir: &Path, filename: &str) -> String {
    lists_dir.join(filename).to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategy::schema::{DesyncMode, FakeTlsCfg, SplitMarker};
    use std::path::PathBuf;

    fn sample_params() -> BypassParams {
        let mut tcp_leg = DesyncLeg::new(LegProto::Tcp, "443");
        tcp_leg.hosts.hostlist_files = vec!["list-general.txt".into()];
        tcp_leg.desync_mode = vec![DesyncMode::Multisplit];
        tcp_leg.split_position = vec![SplitMarker::Fixed(1), SplitMarker::Midsld];
        tcp_leg.repeats = 6;
        tcp_leg.fooling = vec!["md5sig".into()];
        tcp_leg.fake_tls = vec![FakeTlsCfg {
            sni_domain: "www.google.com".into(),
            fake_data_hex: Some("aabbcc".into()),
        }];

        let mut udp_leg = DesyncLeg::new(LegProto::Udp, "443");
        udp_leg.desync_mode = vec![DesyncMode::Fake];
        udp_leg.repeats = 6;
        udp_leg.fake_quic_hex = vec!["ddeeff".into()];

        BypassParams {
            legs: vec![tcp_leg, udp_leg],
        }
    }

    #[test]
    fn builds_wf_filters_and_new_separated_legs() {
        let params = sample_params();
        let hostlist = PathBuf::from(r"C:\fake\lists\hydra-target.txt");
        let args = build_winws_args(&params, &[], &hostlist);

        assert_eq!(args[0], "--wf-tcp=443");
        assert_eq!(args[1], "--wf-udp=443");
        assert!(args.contains(&"--new".to_string()));
        assert!(args.contains(&"--filter-tcp=443".to_string()));
        assert!(args.contains(&"--filter-udp=443".to_string()));
    }

    #[test]
    fn resolves_hostlist_files_relative_to_hostlist_parent() {
        let params = sample_params();
        let hostlist = PathBuf::from(r"C:\fake\lists\hydra-target.txt");
        let args = build_winws_args(&params, &[], &hostlist);
        assert!(args
            .iter()
            .any(|a| a == r"--hostlist=C:\fake\lists\list-general.txt"));
    }

    #[test]
    fn injects_target_hostlist_path_when_hosts_present() {
        let params = sample_params();
        let hostlist = PathBuf::from(r"C:\fake\lists\hydra-target.txt");
        let args = build_winws_args(&params, &["discord.com".into()], &hostlist);
        assert!(args
            .iter()
            .any(|a| a == r"--hostlist=C:\fake\lists\hydra-target.txt"));
    }

    #[test]
    fn emits_combined_desync_mode_and_split_pos() {
        let params = sample_params();
        let hostlist = PathBuf::from(r"C:\fake\lists\hydra-target.txt");
        let args = build_winws_args(&params, &[], &hostlist);
        assert!(args.contains(&"--dpi-desync=multisplit".to_string()));
        assert!(args.contains(&"--dpi-desync-split-pos=1,midsld".to_string()));
        assert!(args.contains(&"--dpi-desync-fake-tls=0xaabbcc".to_string()));
        assert!(args.contains(&"--dpi-desync-fake-quic=0xddeeff".to_string()));
    }
}
