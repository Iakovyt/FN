//! Data model for a bypass strategy. Real winws.exe strategies (see
//! `resources/zapret/general*.bat`) chain several `--new`-separated legs,
//! each targeting a different protocol/port range with its own desync
//! settings — so `BypassParams` holds `Vec<DesyncLeg>` rather than one flat
//! set of fields. Every field here was extracted from the bundled `.bat`
//! files, not invented; see `bin/import_strategies.rs`.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TargetProtocol {
    Discord,
    DiscordVoice,
    YouTube,
    Telegram,
    Generic(String),
}

impl TargetProtocol {
    pub fn as_db_str(&self) -> String {
        match self {
            TargetProtocol::Discord => "discord".into(),
            TargetProtocol::DiscordVoice => "discord_voice".into(),
            TargetProtocol::YouTube => "youtube".into(),
            TargetProtocol::Telegram => "telegram".into(),
            TargetProtocol::Generic(name) => format!("generic:{name}"),
        }
    }

    pub fn from_db_str(value: &str) -> Self {
        match value {
            "discord" => TargetProtocol::Discord,
            "discord_voice" => TargetProtocol::DiscordVoice,
            "youtube" => TargetProtocol::YouTube,
            "telegram" => TargetProtocol::Telegram,
            other => TargetProtocol::Generic(
                other.strip_prefix("generic:").unwrap_or(other).to_string(),
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StrategySource {
    Builtin,
    Community,
    UserCustom,
}

impl StrategySource {
    pub fn as_db_str(&self) -> &'static str {
        match self {
            StrategySource::Builtin => "builtin",
            StrategySource::Community => "community",
            StrategySource::UserCustom => "user_custom",
        }
    }

    pub fn from_db_str(value: &str) -> Self {
        match value {
            "community" => StrategySource::Community,
            "user_custom" => StrategySource::UserCustom,
            _ => StrategySource::Builtin,
        }
    }
}

/// `--dpi-desync=` accepts a comma-separated combination of methods (e.g.
/// `fake,multidisorder`), so this is a set, not a single choice. Every
/// variant here is a literal token observed in a bundled `general*.bat`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DesyncMode {
    Fake,
    Multisplit,
    Multidisorder,
    #[serde(rename = "fakedsplit")]
    FakeDSplit,
    #[serde(rename = "hostfakesplit")]
    HostFakeSplit,
    #[serde(rename = "syndata")]
    SynData,
}

impl DesyncMode {
    pub fn token(self) -> &'static str {
        match self {
            DesyncMode::Fake => "fake",
            DesyncMode::Multisplit => "multisplit",
            DesyncMode::Multidisorder => "multidisorder",
            DesyncMode::FakeDSplit => "fakedsplit",
            DesyncMode::HostFakeSplit => "hostfakesplit",
            DesyncMode::SynData => "syndata",
        }
    }
}

/// `--dpi-desync-split-pos=` also combines markers, e.g. `1,midsld`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SplitMarker {
    Fixed(u16),
    Sni,
    Midsld,
    /// `sniext`, optionally offset (`sniext+1`, `sniext-2`): a position
    /// relative to the end of the TLS SNI extension.
    #[serde(rename = "sniext")]
    SniExt(i32),
}

impl SplitMarker {
    pub fn token(&self) -> String {
        match self {
            SplitMarker::Fixed(n) => n.to_string(),
            SplitMarker::Sni => "sni".into(),
            SplitMarker::Midsld => "midsld".into(),
            SplitMarker::SniExt(0) => "sniext".into(),
            SplitMarker::SniExt(offset) if *offset > 0 => format!("sniext+{offset}"),
            SplitMarker::SniExt(offset) => format!("sniext{offset}"),
        }
    }
}

pub type SplitPos = Vec<SplitMarker>;

/// One `--dpi-desync-fake-tls` value. Builtin strategies reference a file
/// under `bin/`; we store that file's real bytes hex-encoded so the JSON is
/// self-contained and doesn't depend on a specific zapret install layout.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct FakeTlsCfg {
    pub sni_domain: String,
    pub fake_data_hex: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LegProto {
    Tcp,
    Udp,
}

impl LegProto {
    pub fn token(self) -> &'static str {
        match self {
            LegProto::Tcp => "tcp",
            LegProto::Udp => "udp",
        }
    }
}

/// Hostlist/ipset selectors for one leg, mirroring `--hostlist*`/`--ipset*`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct HostMatch {
    pub hostlist_files: Vec<String>,
    pub hostlist_exclude_files: Vec<String>,
    pub hostlist_domains: Vec<String>,
    pub ipset_files: Vec<String>,
    pub ipset_exclude_files: Vec<String>,
}

impl HostMatch {
    pub fn is_empty(&self) -> bool {
        self.hostlist_files.is_empty()
            && self.hostlist_exclude_files.is_empty()
            && self.hostlist_domains.is_empty()
            && self.ipset_files.is_empty()
            && self.ipset_exclude_files.is_empty()
    }
}

/// One `--filter-tcp`/`--filter-udp` ... `--new` block of a winws command
/// line. `ports` keeps `%GameFilterTCP%`/`%GameFilterUDP%` tokens verbatim
/// when present — the same placeholders the existing `strategies.rs`
/// resolves at spawn time via `game_filter_ports`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DesyncLeg {
    pub proto: LegProto,
    pub ports: String,
    /// `--filter-l3=ipv4|ipv6`, when the leg restricts to one IP version.
    pub filter_l3: Option<String>,
    pub filter_l7: Vec<String>,
    pub hosts: HostMatch,
    pub desync_mode: Vec<DesyncMode>,
    pub split_position: SplitPos,
    pub fake_tls: Vec<FakeTlsCfg>,
    /// Raw `--dpi-desync-fake-tls-mod` value (e.g. `rnd,dupsid,sni=host`)
    /// for winws's own auto-generated fake ClientHello, distinct from a
    /// pre-baked blob in `fake_tls`.
    pub fake_tls_mod: Option<String>,
    /// `--dpi-desync-fakedsplit-pattern`, a short inline hex byte pattern
    /// used with `DesyncMode::FakeDSplit`.
    pub fakedsplit_pattern_hex: Option<String>,
    pub ttl: Option<u8>,
    pub repeats: u8,
    /// Reserved for a future synthetic-UDP-fake-payload length; no bundled
    /// strategy uses a length-only UDP fake (they all reference a file via
    /// `fake_unknown_udp_hex`), so `winws_mapper` does not emit a flag for
    /// this yet.
    pub udp_fake_len: Option<u16>,
    pub autottl: bool,
    pub split_seqovl: Option<u16>,
    pub split_seqovl_pattern_hex: Option<String>,
    pub fooling: Vec<String>,
    pub badseq_increment: Option<i64>,
    pub fake_quic_hex: Vec<String>,
    pub fake_discord_hex: Vec<String>,
    pub fake_stun_hex: Vec<String>,
    pub fake_http_hex: Vec<String>,
    pub fake_unknown_udp_hex: Vec<String>,
    pub hostfakesplit_host: Option<String>,
    pub any_protocol: bool,
    pub cutoff: Option<String>,
    pub ip_id_zero: bool,
}

impl DesyncLeg {
    pub fn new(proto: LegProto, ports: impl Into<String>) -> Self {
        Self {
            proto,
            ports: ports.into(),
            filter_l3: None,
            filter_l7: Vec::new(),
            hosts: HostMatch::default(),
            desync_mode: Vec::new(),
            split_position: Vec::new(),
            fake_tls: Vec::new(),
            fake_tls_mod: None,
            fakedsplit_pattern_hex: None,
            ttl: None,
            repeats: 1,
            udp_fake_len: None,
            autottl: false,
            split_seqovl: None,
            split_seqovl_pattern_hex: None,
            fooling: Vec::new(),
            badseq_increment: None,
            fake_quic_hex: Vec::new(),
            fake_discord_hex: Vec::new(),
            fake_stun_hex: Vec::new(),
            fake_http_hex: Vec::new(),
            fake_unknown_udp_hex: Vec::new(),
            hostfakesplit_host: None,
            any_protocol: false,
            cutoff: None,
            ip_id_zero: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct BypassParams {
    pub legs: Vec<DesyncLeg>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Strategy {
    pub id: String,
    pub name: String,
    pub target: TargetProtocol,
    pub params: BypassParams,
    pub source: StrategySource,
    pub created_at: i64,
}

impl Strategy {
    /// `id` is always derived from `params` (sha256 of the canonical JSON
    /// encoding), so two strategies with identical bypass parameters
    /// dedup to the same id regardless of name/target/source.
    pub fn new(
        name: impl Into<String>,
        target: TargetProtocol,
        params: BypassParams,
        source: StrategySource,
        created_at: i64,
    ) -> Self {
        let id = compute_id(&params);
        Self {
            id,
            name: name.into(),
            target,
            params,
            source,
            created_at,
        }
    }
}

pub fn compute_id(params: &BypassParams) -> String {
    let encoded = serde_json::to_vec(params).expect("BypassParams always serializes");
    let mut hasher = Sha256::new();
    hasher.update(&encoded);
    encode_hex(&hasher.finalize())
}

pub fn encode_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

pub fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if value.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(value.len() / 2);
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = (bytes[i] as char).to_digit(16)?;
        let lo = (bytes[i + 1] as char).to_digit(16)?;
        out.push(((hi << 4) | lo) as u8);
        i += 2;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_is_deterministic_and_ignores_name_target_source() {
        let params = BypassParams {
            legs: vec![DesyncLeg::new(LegProto::Tcp, "443")],
        };
        let a = Strategy::new(
            "A",
            TargetProtocol::Discord,
            params.clone(),
            StrategySource::Builtin,
            0,
        );
        let b = Strategy::new(
            "B",
            TargetProtocol::YouTube,
            params,
            StrategySource::UserCustom,
            123,
        );
        assert_eq!(a.id, b.id);
        assert_eq!(a.id.len(), 64);
    }

    #[test]
    fn id_changes_when_params_change() {
        let a = compute_id(&BypassParams {
            legs: vec![DesyncLeg::new(LegProto::Tcp, "443")],
        });
        let b = compute_id(&BypassParams {
            legs: vec![DesyncLeg::new(LegProto::Udp, "443")],
        });
        assert_ne!(a, b);
    }

    #[test]
    fn hex_roundtrip() {
        let bytes = [0x00u8, 0x1a, 0xff, 0x42];
        let hex = encode_hex(&bytes);
        assert_eq!(hex, "001aff42");
        assert_eq!(decode_hex(&hex).unwrap(), bytes.to_vec());
    }
}
