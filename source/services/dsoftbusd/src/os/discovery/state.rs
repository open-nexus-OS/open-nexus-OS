// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Lightweight deterministic peer-IP map helpers for discovery (set_peer_ip, get_peer_ip) with const port and multicast IP defaults.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No tests
//! ADR: docs/adr/0005-dsoftbus-architecture.md
//! Lightweight deterministic peer-IP map helpers.

use alloc::string::String;
use alloc::vec::Vec;

pub(crate) const DISC_PORT: u16 = 37_020;
pub(crate) const MCAST_IP: [u8; 4] = [239, 42, 0, 1];

#[inline]
pub(crate) fn set_peer_ip(ips: &mut Vec<(String, [u8; 4])>, id: &str, ip: [u8; 4]) {
    if let Some(pos) = ips.iter().position(|(x, _)| x == id) {
        ips[pos].1 = ip;
    } else {
        ips.push((String::from(id), ip));
    }
}

#[inline]
pub(crate) fn get_peer_ip(ips: &[(String, [u8; 4])], id: &str) -> Option<[u8; 4]> {
    ips.iter().find(|(x, _)| x == id).map(|(_, ip)| *ip)
}
