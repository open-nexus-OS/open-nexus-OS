// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Reply frame helpers for netstackd IPC operations
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Planned in netstackd host seam tests
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#[cfg(test)]
use super::wire::{MAGIC0, MAGIC1, VERSION};
#[cfg(not(test))]
use crate::os::ipc::wire::{MAGIC0, MAGIC1, VERSION};

#[inline]
pub(crate) fn append_nonce(out: &mut [u8], nonce: u64) {
    out.copy_from_slice(&nonce.to_le_bytes());
}

#[inline]
pub(crate) fn status_frame(op: u8, status: u8) -> [u8; 5] {
    [MAGIC0, MAGIC1, VERSION, op | 0x80, status]
}

/// Writes the 5-byte reply header into `out` (must be at least 5 bytes).
#[inline]
pub(crate) fn fill_header_prefix(out: &mut [u8], op: u8, status: u8) {
    out[..5].copy_from_slice(&status_frame(op, status));
}

/// Status-only reply, with optional 8-byte nonce tail (13 bytes total when nonce is present).
#[inline]
pub(crate) fn reply_status_maybe_nonce<R: FnMut(&[u8]) + ?Sized>(
    reply: &mut R,
    op: u8,
    status: u8,
    nonce: Option<u64>,
) {
    match nonce {
        Some(n) => {
            let mut rsp = [0u8; 13];
            fill_header_prefix(&mut rsp, op, status);
            append_nonce(&mut rsp[5..13], n);
            reply(&rsp);
        }
        None => reply(&status_frame(op, status)),
    }
}

/// Status + little-endian `u32` payload (e.g. stream/listener id), optional nonce.
#[inline]
pub(crate) fn reply_u32_status_maybe_nonce<R: FnMut(&[u8]) + ?Sized>(
    reply: &mut R,
    op: u8,
    status: u8,
    value: u32,
    nonce: Option<u64>,
) {
    match nonce {
        Some(n) => {
            let mut rsp = [0u8; 17];
            fill_header_prefix(&mut rsp, op, status);
            rsp[5..9].copy_from_slice(&value.to_le_bytes());
            append_nonce(&mut rsp[9..17], n);
            reply(&rsp);
        }
        None => {
            let mut rsp = [0u8; 9];
            fill_header_prefix(&mut rsp, op, status);
            rsp[5..9].copy_from_slice(&value.to_le_bytes());
            reply(&rsp);
        }
    }
}

/// Status + little-endian `u16` field (e.g. byte count, RTT), optional nonce.
#[inline]
pub(crate) fn reply_u16_field_status_maybe_nonce<R: FnMut(&[u8]) + ?Sized>(
    reply: &mut R,
    op: u8,
    status: u8,
    field: u16,
    nonce: Option<u64>,
) {
    match nonce {
        Some(n) => {
            let mut rsp = [0u8; 15];
            fill_header_prefix(&mut rsp, op, status);
            rsp[5..7].copy_from_slice(&field.to_le_bytes());
            append_nonce(&mut rsp[7..15], n);
            reply(&rsp);
        }
        None => {
            let mut rsp = [0u8; 7];
            fill_header_prefix(&mut rsp, op, status);
            rsp[5..7].copy_from_slice(&field.to_le_bytes());
            reply(&rsp);
        }
    }
}

/// Status + little-endian `u16` length + payload bytes, optional nonce.
#[inline]
pub(crate) fn reply_u16_len_payload_status_maybe_nonce<R: FnMut(&[u8]) + ?Sized>(
    reply: &mut R,
    op: u8,
    status: u8,
    payload: &[u8],
    nonce: Option<u64>,
) {
    debug_assert!(payload.len() <= 480);
    let mut rsp = [0u8; 512];
    fill_header_prefix(&mut rsp, op, status);
    let n = payload.len();
    rsp[5..7].copy_from_slice(&(n as u16).to_le_bytes());
    rsp[7..7 + n].copy_from_slice(payload);
    if let Some(nonce) = nonce {
        let end = 7 + n;
        append_nonce(&mut rsp[end..end + 8], nonce);
        reply(&rsp[..end + 8]);
    } else {
        reply(&rsp[..7 + n]);
    }
}

/// Status + little-endian `u16` length + source IPv4 + source port + payload, optional nonce.
#[inline]
pub(crate) fn reply_u16_len_ipv4_port_payload_status_maybe_nonce<R: FnMut(&[u8]) + ?Sized>(
    reply: &mut R,
    op: u8,
    status: u8,
    source_ip: [u8; 4],
    source_port: u16,
    payload: &[u8],
    nonce: Option<u64>,
) {
    debug_assert!(payload.len() <= 460);
    let mut rsp = [0u8; 512];
    fill_header_prefix(&mut rsp, op, status);
    let n = payload.len();
    rsp[5..7].copy_from_slice(&(n as u16).to_le_bytes());
    rsp[7..11].copy_from_slice(&source_ip);
    rsp[11..13].copy_from_slice(&source_port.to_le_bytes());
    rsp[13..13 + n].copy_from_slice(payload);
    if let Some(nonce) = nonce {
        let end = 13 + n;
        append_nonce(&mut rsp[end..end + 8], nonce);
        reply(&rsp[..end + 8]);
    } else {
        reply(&rsp[..13 + n]);
    }
}
