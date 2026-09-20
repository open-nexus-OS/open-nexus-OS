// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: writing to the UART without allocating, split out of `lib.rs`
//! under the structure ratchet (TASK-0077C P1).
//!
//! Every caller here runs somewhere allocation is impossible or unwise: inside
//! the global allocator (the watermark and failure lines), inside the panic
//! handler, and inside the alloc-error handler. That is the whole reason these
//! exist instead of `format!` — and the reason they live together.
//!
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: exercised by every OS boot; no host path (`debug_putc` is a syscall)

#![cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]

use nexus_abi::debug_putc;

pub(crate) fn debug_write_bytes(bytes: &[u8]) {
    for &byte in bytes {
        let _ = debug_putc(byte);
    }
}

pub(crate) fn debug_write_byte(byte: u8) {
    let _ = debug_putc(byte);
}

pub(crate) fn debug_write_str(s: &str) {
    debug_write_bytes(s.as_bytes());
}

pub(crate) fn debug_write_hex(mut value: usize) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut buf = [0u8; core::mem::size_of::<usize>() * 2];
    for idx in (0..buf.len()).rev() {
        buf[idx] = HEX[(value & 0xF) as usize];
        value >>= 4;
    }
    for byte in &buf {
        let _ = debug_putc(*byte);
    }
}

pub(crate) fn debug_write_dec(mut value: u64) {
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
    for byte in &buf[idx..] {
        let _ = debug_putc(*byte);
    }
}
