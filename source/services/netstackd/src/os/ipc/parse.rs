// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: IPC parse helpers for netstackd request and reply frames
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Planned in netstackd host seam tests
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#[inline]
pub(crate) fn parse_nonce(req: &[u8], base_len: usize) -> Option<u64> {
    if req.len() == base_len + 8 {
        let mut b = [0u8; 8];
        b.copy_from_slice(&req[base_len..base_len + 8]);
        Some(u64::from_le_bytes(b))
    } else {
        None
    }
}

#[inline]
pub(crate) fn parse_u32_le(req: &[u8], start: usize) -> Option<u32> {
    req.get(start..start + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// Little-endian `u16` at `start` (inclusive). Returns `None` if the slice is too short.
#[inline]
pub(crate) fn parse_u16_le(req: &[u8], start: usize) -> Option<u16> {
    req.get(start..start + 2).map(|s| u16::from_le_bytes([s[0], s[1]]))
}

/// Four-byte IPv4 octets at `start`. Returns `None` if the slice is too short.
#[inline]
pub(crate) fn parse_ipv4_at(req: &[u8], start: usize) -> Option<[u8; 4]> {
    let s = req.get(start..start + 4)?;
    Some([s[0], s[1], s[2], s[3]])
}

#[inline]
pub(crate) fn has_valid_wire_header(req: &[u8]) -> bool {
    req.len() >= 4
        && req[0] == super::wire::MAGIC0
        && req[1] == super::wire::MAGIC1
        && req[2] == super::wire::VERSION
}
