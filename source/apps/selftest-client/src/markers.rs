// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: UART marker helpers for selftest-client (no-alloc, deterministic output)
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Marker-driven via just test-os
//! ADR: docs/adr/0017-service-architecture.md

#![allow(clippy::missing_docs_in_private_items)]

use nexus_abi::debug_putc;

/// Emit a single byte over UART.
pub fn emit_byte(byte: u8) {
    let _ = debug_putc(byte);
}

/// Emit raw bytes over UART.
pub fn emit_bytes(bytes: &[u8]) {
    for &b in bytes {
        emit_byte(b);
    }
}

/// Emit a newline-terminated marker line over UART.
pub fn emit_line(message: &str) {
    emit_bytes(message.as_bytes());
    emit_byte(b'\n');
}

/// Emit `prefix` followed by `value` as decimal u8 and a newline.
// NOTE: keep this module minimal and only include helpers currently used in
// the marker-driven OS tests (RFC-0003: avoid drifting, duplicated helper sets).

/// Emit an unsigned decimal u64 (no allocation).
pub fn emit_u64(mut value: u64) {
    let mut buf = [0u8; 20];
    let mut idx = buf.len();
    if value == 0 {
        idx -= 1;
        buf[idx] = b'0';
    } else {
        while value != 0 {
            idx -= 1;
            buf[idx] = b'0' + (value % 10) as u8;
            value /= 10;
        }
    }
    emit_bytes(&buf[idx..]);
}

/// Emit a signed decimal i64 (no allocation).
pub fn emit_i64(value: i64) {
    if value < 0 {
        emit_byte(b'-');
        emit_u64((-value) as u64);
    } else {
        emit_u64(value as u64);
    }
}

/// Emit a fixed-width 16-nybble hex u64 (no allocation).
pub fn emit_hex_u64(mut value: u64) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut buf = [0u8; 16];
    for idx in (0..buf.len()).rev() {
        buf[idx] = HEX[(value & 0xF) as usize];
        value >>= 4;
    }
    emit_bytes(&buf);
}
