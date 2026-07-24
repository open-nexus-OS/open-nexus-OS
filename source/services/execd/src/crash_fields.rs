// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: pure `key=value\n` byte serializers for execd's crash / minidump
//! records, split out of `os_lite.rs` (module-size ratchet). No state, no I/O —
//! just decimal formatting into a caller `Vec<u8>`. Used by the crash-log append
//! and minidump-artifact paths.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: exercised via the crash/minidump QEMU markers

extern crate alloc;

use alloc::vec::Vec;

pub(crate) fn push_u32_dec(out: &mut Vec<u8>, mut value: u32) {
    let mut tmp = [0u8; 10];
    let mut i = tmp.len();
    if value == 0 {
        out.push(b'0');
        return;
    }
    while value != 0 && i != 0 {
        let digit = (value % 10) as u8;
        value /= 10;
        i -= 1;
        tmp[i] = b'0' + digit;
    }
    out.extend_from_slice(&tmp[i..]);
}

pub(crate) fn push_i32_dec(out: &mut Vec<u8>, value: i32) {
    if value < 0 {
        out.push(b'-');
        push_u32_dec(out, (-value) as u32);
    } else {
        push_u32_dec(out, value as u32);
    }
}

pub(crate) fn append_field(out: &mut Vec<u8>, key: &[u8], value: &[u8]) {
    out.extend_from_slice(key);
    out.extend_from_slice(value);
    out.push(b'\n');
}

pub(crate) fn append_field_u32(out: &mut Vec<u8>, key: &[u8], value: u32) {
    out.extend_from_slice(key);
    push_u32_dec(out, value);
    out.push(b'\n');
}

pub(crate) fn append_field_u64(out: &mut Vec<u8>, key: &[u8], mut value: u64) {
    out.extend_from_slice(key);
    let mut tmp = [0u8; 20];
    let mut i = tmp.len();
    if value == 0 {
        out.push(b'0');
        out.push(b'\n');
        return;
    }
    while value != 0 && i != 0 {
        i -= 1;
        tmp[i] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    out.extend_from_slice(&tmp[i..]);
    out.push(b'\n');
}

pub(crate) fn append_field_i32(out: &mut Vec<u8>, key: &[u8], value: i32) {
    out.extend_from_slice(key);
    push_i32_dec(out, value);
    out.push(b'\n');
}
