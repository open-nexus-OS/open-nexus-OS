// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Pure helper logic for host-testable netstackd seams
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Planned in netstackd host seam tests
//! ADR: docs/adr/0005-dsoftbus-architecture.md

pub(crate) const QEMU_USERNET_FALLBACK_IP: [u8; 4] = [10, 0, 2, 15];
pub(crate) const QEMU_USERNET_GATEWAY_IP: [u8; 4] = [10, 0, 2, 2];
pub(crate) const QEMU_USERNET_DNS_PRIMARY_IP: [u8; 4] = [10, 0, 2, 3];
pub(crate) const OS2VM_STATIC_PREFIX: [u8; 3] = [10, 42, 0];
pub(crate) const OS2VM_NODE_A_IP: [u8; 4] = [10, 42, 0, 10];
pub(crate) const OS2VM_NODE_B_IP: [u8; 4] = [10, 42, 0, 11];
pub(crate) const DNS_SERVER_PORT: u16 = 53;
const DNS_PROBE_TXID_HI: u8 = 0x12;
const DNS_PROBE_TXID_LO: u8 = 0x34;

#[inline]
pub(crate) fn fallback_ipv4_config(
    is_qemu_smoke: bool,
    mac: [u8; 6],
) -> ([u8; 4], u8, Option<[u8; 4]>) {
    if is_qemu_smoke {
        // QEMU usernet-compatible static fallback.
        (QEMU_USERNET_FALLBACK_IP, 24u8, Some(QEMU_USERNET_GATEWAY_IP))
    } else {
        // Deterministic 2-VM fallback from NIC MAC LSB.
        let host = if mac[5] == 0 { 1 } else { mac[5] };
        let ip = match host {
            10 => OS2VM_NODE_A_IP,
            11 => OS2VM_NODE_B_IP,
            _ => [OS2VM_STATIC_PREFIX[0], OS2VM_STATIC_PREFIX[1], OS2VM_STATIC_PREFIX[2], host],
        };
        (ip, 24u8, None)
    }
}

#[inline]
pub(crate) fn is_qemu_loopback_target(ip: [u8; 4], port: u16, a: u16, b: u16) -> bool {
    ip == QEMU_USERNET_FALLBACK_IP && (port == a || port == b)
}

/// Where a `connect`/`listen` target lives (RFC-0092 facade prerequisite:
/// an in-facade loopback that never touches the NIC).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LocalTarget {
    /// `127/8`: loopback-only (a listener there is unreachable from the NIC).
    Loopback,
    /// The interface's own address: hairpin onto the local listener.
    OwnIp,
    /// Anything else goes to the stack.
    Remote,
}

#[inline]
pub(crate) fn is_loopback_ip(ip: [u8; 4]) -> bool {
    ip[0] == 127
}

/// Classifies `ip` against the interface address `own_ip`.
#[inline]
pub(crate) fn local_target(ip: [u8; 4], own_ip: [u8; 4]) -> LocalTarget {
    if is_loopback_ip(ip) {
        LocalTarget::Loopback
    } else if ip == own_ip {
        LocalTarget::OwnIp
    } else {
        LocalTarget::Remote
    }
}

/// The address the accepting side sees for a hairpinned/loopback connect:
/// the connector's own address (loopback stays loopback) and a synthetic
/// ephemeral port derived from the connector's stream slot.
#[inline]
pub(crate) fn loop_remote_for_acceptor(
    target: LocalTarget,
    own_ip: [u8; 4],
    connector_index: usize,
) -> ([u8; 4], u16) {
    let ip = match target {
        LocalTarget::Loopback => [127, 0, 0, 1],
        _ => own_ip,
    };
    (ip, 49_152u16.wrapping_add((connector_index % 16_384) as u16))
}

#[inline]
pub(crate) fn is_dns_probe_response(frame: &[u8], from_port: u16) -> bool {
    frame.len() >= 12
        && from_port == DNS_SERVER_PORT
        && frame[0] == DNS_PROBE_TXID_HI
        && frame[1] == DNS_PROBE_TXID_LO
        && (frame[2] & 0x80) != 0
}
