//! Minimal network-change signal for Hydra's rescan trigger.
//!
//! The task spec suggested the `windows` crate (`GetAdaptersAddresses` /
//! `NotifyIpInterfaceChange`) for this. That's a real, heavier dependency
//! this project doesn't otherwise need; polling the OS routing table for
//! "which local IP would a new connection leave from" is a much lighter
//! substitute that's good enough for a change *signal* (as opposed to a
//! full adapter inventory), so that's what this does instead.

use std::net::IpAddr;

/// Connecting a UDP socket doesn't send any packets — it just asks the OS
/// to pick a route and bind a local endpoint — so this is a read-only
/// routing-table lookup, not real network traffic. Returns `None` if the
/// machine currently has no route at all (e.g. fully offline).
pub fn primary_local_ip() -> Option<IpAddr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("1.1.1.1:80").ok()?;
    socket.local_addr().ok().map(|addr| addr.ip())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_local_ip_does_not_panic() {
        // Environment-dependent (may be None when fully offline); this
        // just guards against a panic in the lookup itself.
        let _ = primary_local_ip();
    }
}
