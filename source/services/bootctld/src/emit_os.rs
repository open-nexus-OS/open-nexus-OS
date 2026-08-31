// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]
#![forbid(unsafe_code)]

//! CONTEXT: bootctld's fixed-buffer marker renderers (split from
//! `os_lite.rs` under the structure ratchet). Every line here is a PROOF
//! surface consumed by the QEMU ladder, so the numbers are rendered into
//! stack buffers — the service runs on a bump heap that never frees, and a
//! per-marker `format!` would leak for the lifetime of the process.
//! OWNERS: @reliability
//! STATUS: Functional
//! API_STABILITY: Internal (marker literals are the contract surface)
//! TEST_COVERAGE: QEMU ladder (quorum / floor markers gated)
//! ADR: docs/adr/0055-bootctld-single-boot-state-authority.md

use crate::os_lite::emit;

/// `bootctld: rollback-min held (floor=<n> staged=<m>)` — the honest
/// counterpart of the raise line.
pub(crate) fn emit_floor_held(floor: u32, staged: u32) {
    let mut line = [0u8; 56];
    let head = b"bootctld: rollback-min held (floor=";
    let mut len = head.len();
    line[..len].copy_from_slice(head);
    len += fmt_u32(&mut line[len..], floor);
    line[len..len + 8].copy_from_slice(b" staged=");
    len += 8;
    len += fmt_u32(&mut line[len..], staged);
    line[len] = b')';
    len += 1;
    if let Ok(msg) = core::str::from_utf8(&line[..len]) {
        emit(msg);
    }
}

/// `bootctld: rollback-min raised (<old>-><new>)` — fixed-buffer render.
pub(crate) fn emit_floor_raised(old: u32, new: u32) {
    let mut line = [0u8; 56];
    let head = b"bootctld: rollback-min raised (";
    let mut len = head.len();
    line[..len].copy_from_slice(head);
    len += fmt_u32(&mut line[len..], old);
    line[len..len + 2].copy_from_slice(b"->");
    len += 2;
    len += fmt_u32(&mut line[len..], new);
    line[len] = b')';
    len += 1;
    if let Ok(msg) = core::str::from_utf8(&line[..len]) {
        emit(msg);
    }
}

pub(crate) fn fmt_u32(out: &mut [u8], value: u32) -> usize {
    let mut digits = [0u8; 10];
    let mut n = value;
    let mut count = 0usize;
    loop {
        digits[count] = b'0' + (n % 10) as u8;
        count += 1;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    for i in 0..count {
        out[i] = digits[count - 1 - i];
    }
    count
}

/// `bootctld: health quorum ok (n/n)` — bounded formatting (n <= 8).
pub(crate) fn emit_quorum_ok(have: u8, need: u8) {
    let mut line = [0u8; 40];
    let head = b"bootctld: health quorum ok (";
    let mut len = head.len();
    line[..len].copy_from_slice(head);
    line[len] = b'0' + have.min(8);
    line[len + 1] = b'/';
    line[len + 2] = b'0' + need.min(8);
    line[len + 3] = b')';
    len += 4;
    if let Ok(msg) = core::str::from_utf8(&line[..len]) {
        emit(msg);
    }
}
