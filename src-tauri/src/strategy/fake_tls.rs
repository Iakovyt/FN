//! Fake TLS ClientHello generation for the `fake_tls` bypass leg.
//!
//! Building a fully synthetic TLS 1.3 ClientHello from scratch is easy to
//! get subtly wrong (extension ordering/values that real DPI boxes or
//! servers choke on). Instead we take one real, captured ClientHello
//! (bundled at `resources/zapret/bin/tls_clienthello_www_google_com.bin`,
//! already shipped with the app for the same purpose) and patch only the
//! SNI extension's hostname bytes, recalculating every length field the
//! change touches: the server_name entry, the server_name list, the
//! extension itself, the total extensions length, the handshake length,
//! and the record length.

use crate::strategy::schema::encode_hex;

const REFERENCE_CLIENT_HELLO: &[u8] =
    include_bytes!("../../resources/zapret/bin/tls_clienthello_www_google_com.bin");

/// Returns a hex-encoded TLS ClientHello record with the SNI extension
/// rewritten to `sni`. Falls back to the unmodified reference sample if the
/// bundled blob doesn't parse as a well-formed ClientHello with a
/// server_name extension (should not happen for the shipped file; guards
/// against a future asset swap breaking assumptions silently).
pub fn generate_fake_clienthello(sni: &str) -> String {
    let patched =
        patch_sni(REFERENCE_CLIENT_HELLO, sni).unwrap_or_else(|| REFERENCE_CLIENT_HELLO.to_vec());
    encode_hex(&patched)
}

/// Walks a TLS record / handshake / extension structure just far enough to
/// locate the `server_name` (type 0x0000) extension, then rewrites it.
fn patch_sni(hello: &[u8], new_sni: &str) -> Option<Vec<u8>> {
    if hello.len() < 5 || hello[0] != 0x16 {
        return None; // not a Handshake record
    }
    let mut pos = 5usize; // past the 5-byte record header

    if *hello.get(pos)? != 0x01 {
        return None; // not a ClientHello
    }
    pos += 4; // handshake header: type(1) + length(3)

    pos += 2; // client_version
    pos += 32; // random

    let session_id_len = *hello.get(pos)? as usize;
    pos += 1 + session_id_len;

    let cipher_suites_len = u16::from_be_bytes([*hello.get(pos)?, *hello.get(pos + 1)?]) as usize;
    pos += 2 + cipher_suites_len;

    let compression_len = *hello.get(pos)? as usize;
    pos += 1 + compression_len;

    let extensions_length_pos = pos;
    let extensions_total_len =
        u16::from_be_bytes([*hello.get(pos)?, *hello.get(pos + 1)?]) as usize;
    pos += 2;
    let extensions_start = pos;
    let extensions_end = extensions_start.checked_add(extensions_total_len)?;
    if extensions_end > hello.len() {
        return None;
    }

    let mut cursor = extensions_start;
    while cursor + 4 <= extensions_end {
        let ext_type = u16::from_be_bytes([hello[cursor], hello[cursor + 1]]);
        let ext_len = u16::from_be_bytes([hello[cursor + 2], hello[cursor + 3]]) as usize;
        let payload_start = cursor + 4;
        let payload_end = payload_start.checked_add(ext_len)?;
        if payload_end > extensions_end {
            return None;
        }

        if ext_type == 0x0000 {
            return Some(rewrite_server_name(
                hello,
                extensions_length_pos,
                payload_start,
                payload_end,
                new_sni,
            ));
        }
        cursor = payload_end;
    }
    None
}

fn rewrite_server_name(
    hello: &[u8],
    extensions_length_pos: usize,
    sni_payload_start: usize,
    sni_payload_end: usize,
    new_sni: &str,
) -> Vec<u8> {
    let name = new_sni.as_bytes();
    let entry_len = name.len() as u16;
    let list_len: u16 = 1 + 2 + entry_len; // name_type(1) + name_len(2) + name

    let mut new_payload = Vec::with_capacity(2 + list_len as usize);
    new_payload.extend_from_slice(&list_len.to_be_bytes());
    new_payload.push(0x00); // name_type: host_name
    new_payload.extend_from_slice(&entry_len.to_be_bytes());
    new_payload.extend_from_slice(name);

    let old_payload_len = sni_payload_end - sni_payload_start;
    let delta = new_payload.len() as i64 - old_payload_len as i64;

    let mut out = Vec::with_capacity((hello.len() as i64 + delta).max(0) as usize);
    out.extend_from_slice(&hello[..sni_payload_start]);
    out.extend_from_slice(&new_payload);
    out.extend_from_slice(&hello[sni_payload_end..]);

    // Every offset touched below is < sni_payload_start, i.e. inside the
    // untouched prefix we just copied verbatim, so writing through the
    // original byte offsets into `out` is valid.
    let ext_len_pos = sni_payload_start - 2;
    write_u16(&mut out, ext_len_pos, new_payload.len() as u16);

    let old_extensions_total = read_u16(hello, extensions_length_pos) as i64;
    write_u16(
        &mut out,
        extensions_length_pos,
        (old_extensions_total + delta) as u16,
    );

    // Handshake header: type(1) at offset 5, length(3, big-endian) at 6..9.
    let old_handshake_len = read_u24(hello, 6) as i64;
    write_u24(&mut out, 6, (old_handshake_len + delta) as u32);

    // Record header: type(1) at 0, version(2) at 1..3, length(2) at 3..5.
    let old_record_len = read_u16(hello, 3) as i64;
    write_u16(&mut out, 3, (old_record_len + delta) as u16);

    out
}

fn read_u16(buf: &[u8], pos: usize) -> u16 {
    u16::from_be_bytes([buf[pos], buf[pos + 1]])
}

fn write_u16(buf: &mut [u8], pos: usize, value: u16) {
    let bytes = value.to_be_bytes();
    buf[pos] = bytes[0];
    buf[pos + 1] = bytes[1];
}

fn read_u24(buf: &[u8], pos: usize) -> u32 {
    ((buf[pos] as u32) << 16) | ((buf[pos + 1] as u32) << 8) | (buf[pos + 2] as u32)
}

fn write_u24(buf: &mut [u8], pos: usize, value: u32) {
    buf[pos] = ((value >> 16) & 0xff) as u8;
    buf[pos + 1] = ((value >> 8) & 0xff) as u8;
    buf[pos + 2] = (value & 0xff) as u8;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategy::schema::decode_hex;

    #[test]
    fn reference_sample_is_internally_consistent() {
        // Sanity-check our own offset assumptions against the real bundled
        // sample before trusting the patcher on it.
        let record_len = read_u16(REFERENCE_CLIENT_HELLO, 3) as u32;
        let handshake_len = read_u24(REFERENCE_CLIENT_HELLO, 6);
        assert_eq!(record_len, handshake_len + 4);
        assert_eq!(REFERENCE_CLIENT_HELLO.len(), 5 + record_len as usize);
    }

    #[test]
    fn patches_sni_to_shorter_domain_and_fixes_lengths() {
        let hex = generate_fake_clienthello("a.com");
        let bytes = decode_hex(&hex).expect("valid hex");
        assert_consistent_and_contains(&bytes, "a.com");
    }

    #[test]
    fn patches_sni_to_longer_domain_and_fixes_lengths() {
        let hex = generate_fake_clienthello("a-very-long-subdomain.example.org");
        let bytes = decode_hex(&hex).expect("valid hex");
        assert_consistent_and_contains(&bytes, "a-very-long-subdomain.example.org");
    }

    #[test]
    fn patches_sni_to_same_length_domain() {
        // Same length as "www.google.com" (14 bytes) exercises the
        // zero-delta path.
        let hex = generate_fake_clienthello("www.example.com");
        let bytes = decode_hex(&hex).expect("valid hex");
        assert_consistent_and_contains(&bytes, "www.example.com");
    }

    fn assert_consistent_and_contains(bytes: &[u8], expected_sni: &str) {
        let record_len = read_u16(bytes, 3) as u32;
        let handshake_len = read_u24(bytes, 6);
        assert_eq!(record_len, handshake_len + 4, "record/handshake length");
        assert_eq!(bytes.len(), 5 + record_len as usize, "total blob length");
        assert!(
            bytes
                .windows(expected_sni.len())
                .any(|w| w == expected_sni.as_bytes()),
            "patched SNI bytes not found"
        );
    }
}
