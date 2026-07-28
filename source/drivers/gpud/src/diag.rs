// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: gpud boot diagnostics — numeric UART output for the failure paths
//! that a bare marker cannot explain (TASK-0309).
//!
//! gpud's markers were all static strings, so `gpud: resource vmo_map_page
//! fail` said THAT the framebuffer attach failed but never WHICH page, at
//! WHICH virtual address, or with which kernel error. Diagnosing the ordering
//! hazard from a uart log alone was therefore impossible.
//!
//! These helpers are for one-shot failure paths only — never a per-frame
//! path. gpud runs on a bump allocator that never frees, and `debug_putc` is
//! one syscall per byte.
//!
//! OWNERS: @ui
//! STATUS: Functional (TASK-0309)
//! API_STABILITY: Internal
//! TEST_COVERAGE: exercised by the QEMU boot lane when a map fails

#![cfg(all(feature = "os-lite", target_os = "none"))]

use nexus_abi::debug_putc;

fn put(byte: u8) {
    let _ = debug_putc(byte);
}

fn puts(bytes: &[u8]) {
    for &b in bytes {
        put(b);
    }
}

fn hex_u64(value: u64) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for i in (0..16).rev() {
        let nibble = ((value >> (i * 4)) & 0xF) as usize;
        put(HEX[nibble]);
    }
}

/// `gpud: <label> <k0>=0x… <k1>=0x… …` on one line.
///
/// Bounded by construction: at most [`MAX_FIELDS`] pairs, each a fixed-width
/// u64, so the line length is known at the call site.
pub(crate) const MAX_FIELDS: usize = 6;

pub(crate) fn kv_line(label: &[u8], fields: &[(&[u8], u64)]) {
    puts(b"gpud: ");
    puts(label);
    for (name, value) in fields.iter().take(MAX_FIELDS) {
        put(b' ');
        puts(name);
        puts(b"=0x");
        hex_u64(*value);
    }
    put(b'\n');
}

/// Stable short name for an [`nexus_abi::AbiError`], so the log says which
/// kernel rejection happened instead of collapsing every cause into "fail".
///
/// EXHAUSTIVE on purpose — no `_` arm. A catch-all here would recreate the
/// exact defect this module exists to fix: the first cut of it folded five
/// variants into "other" and the boot log said `err=other`, which is a
/// slightly longer way of saying nothing. If `AbiError` grows a variant, this
/// match must fail to compile so somebody names it.
pub(crate) fn abi_error_name(err: nexus_abi::AbiError) -> &'static [u8] {
    use nexus_abi::AbiError as E;
    match err {
        E::InvalidSyscall => b"invalid-syscall",
        E::CapabilityDenied => b"capability-denied",
        E::IpcFailure => b"ipc-failure",
        E::SpawnFailed => b"spawn-failed",
        E::TransferFailed => b"transfer-failed",
        E::ChildUnavailable => b"child-unavailable",
        E::NoSuchPid => b"no-such-pid",
        E::InvalidArgument => b"invalid-argument",
        E::TimedOut => b"timed-out",
        E::WouldBlock => b"would-block",
        E::AlreadyExists => b"already-mapped",
        E::BadAddress => b"bad-address",
        E::Unknown => b"unknown-kernel-code",
        E::Unsupported => b"unsupported",
    }
}

/// `gpud: <label> err=<name>` — the error name is not a hex field, so it gets
/// its own emitter rather than being squeezed into [`kv_line`].
pub(crate) fn err_line(label: &[u8], err: nexus_abi::AbiError) {
    puts(b"gpud: ");
    puts(label);
    puts(b" err=");
    puts(abi_error_name(err));
    put(b'\n');
}
