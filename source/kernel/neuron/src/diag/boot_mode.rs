// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Kernel-side fw_cfg boot-mode probe. The kernel emits its own boot markers
//!   (`[INFO sched]`, `KSELFTEST: …`, address-space/exec traces) BEFORE userspace `init`
//!   runs, so it cannot be told the boot mode by anyone — it must read it itself. This
//!   reads the QEMU `fw_cfg` `opt/org.open-nexus/selftest-mode` key (the SAME source
//!   `selftest-client` uses) once, right after the kernel address space is active (fw_cfg
//!   is identity-mapped in `mm::address_space::map_kernel_segments`).
//!
//!   Purpose: gate whether the kernel FOLDS its boot markers into the verdict grid
//!   (interactive `just start`) or emits them RAW (proof `just test-os`, where
//!   `verify-uart` greps the individual `KSELFTEST:` markers). The default on ANY read
//!   failure is PROOF/raw, so a bad probe can never silently break the proof harness.
//!
//! ALLOC-FREE by construction: fixed stack buffers + MMIO register reads only — no heap,
//! no `Vec`/`String`/`format!`. Register access mirrors the proven userspace reader in
//! `selftest-client::os_lite::boot_cfg` byte-for-byte (SELECTOR at base+8 written big-endian,
//! DATA at base+0).
//!
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable

#![allow(clippy::missing_docs_in_private_items)]

use core::sync::atomic::{AtomicU8, Ordering};

/// QEMU virt `VIRT_FW_CFG` window base (identity-mapped in the kernel AS).
const FW_CFG_BASE: usize = 0x1010_0000;
/// Data register — read file bytes here.
const FW_CFG_DATA: usize = FW_CFG_BASE;
/// Selector register — write the 16-bit (big-endian) key/file selector here.
const FW_CFG_SELECTOR: usize = FW_CFG_BASE + 8;
/// Selector for the fw_cfg signature key.
const FW_CFG_SIGNATURE: u16 = 0x0000;
/// Selector for the file directory.
const FW_CFG_FILE_DIR: u16 = 0x0019;
/// The runtime boot-config key (identical to the userspace reader).
const SELFTEST_MODE_FILE: &[u8] = b"opt/org.open-nexus/selftest-mode";

const MODE_UNKNOWN: u8 = 0;
const MODE_PROOF: u8 = 1;
const MODE_INTERACTIVE: u8 = 2;

/// Resolved boot mode (set once by [`detect`]). Defaults to UNKNOWN, which folds like PROOF
/// (raw markers) so verify-uart is never disturbed by a failed or absent probe.
static BOOT_MODE: AtomicU8 = AtomicU8::new(MODE_UNKNOWN);

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn select(key: u16) {
    // SAFETY: fw_cfg selector register, identity-mapped device page, single 16-bit write.
    unsafe {
        core::ptr::write_volatile(FW_CFG_SELECTOR as *mut u16, key.to_be());
    }
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn read_u8() -> u8 {
    // SAFETY: fw_cfg data register, identity-mapped device page, single byte read.
    unsafe { core::ptr::read_volatile(FW_CFG_DATA as *const u8) }
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn read_be_u16() -> u16 {
    u16::from_be_bytes([read_u8(), read_u8()])
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn read_be_u32() -> u32 {
    u32::from_be_bytes([read_u8(), read_u8(), read_u8(), read_u8()])
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn signature_ok() -> bool {
    select(FW_CFG_SIGNATURE);
    read_u8() == b'Q' && read_u8() == b'E' && read_u8() == b'M' && read_u8() == b'U'
}

/// Walk the fw_cfg file directory and return the selector for `name`, if present.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn find_file(name: &[u8]) -> Option<u16> {
    select(FW_CFG_FILE_DIR);
    let count = read_be_u32();
    // Defensive bound: a sane fw_cfg directory is small; never loop unbounded on garbage.
    if count > 256 {
        return None;
    }
    for _ in 0..count {
        let _size = read_be_u32();
        let selector = read_be_u16();
        let _reserved = read_be_u16();
        let mut fname = [0u8; 56];
        for byte in &mut fname {
            *byte = read_u8();
        }
        let nlen = fname.iter().position(|&c| c == 0).unwrap_or(fname.len());
        if &fname[..nlen] == name {
            return Some(selector);
        }
    }
    None
}

/// Probe the boot mode from fw_cfg ONCE. Must be called after the kernel address space is
/// active (fw_cfg is mapped). Safe to leave UNKNOWN (= raw) on any failure.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub fn detect() {
    if !signature_ok() {
        return;
    }
    let selector = match find_file(SELFTEST_MODE_FILE) {
        Some(sel) => sel,
        None => return,
    };
    select(selector);
    let mut buf = [0u8; 24];
    for byte in &mut buf {
        *byte = read_u8();
    }
    let len = buf
        .iter()
        .position(|&c| c == 0 || c == b'\n' || c == b'\r' || c == b' ')
        .unwrap_or(buf.len());
    let mode = &buf[..len];
    let resolved = if mode == b"proof" {
        MODE_PROOF
    } else if mode.starts_with(b"interactive") {
        MODE_INTERACTIVE
    } else {
        MODE_UNKNOWN
    };
    BOOT_MODE.store(resolved, Ordering::Relaxed);
    let label: &str = match resolved {
        MODE_PROOF => "proof",
        MODE_INTERACTIVE => "interactive",
        _ => "unknown",
    };
    // One-time boot diagnostic so the read is verifiable (will itself fold once the kernel
    // verdict aggregator lands). Single atomic line via the diag facade.
    log_info!(target: "boot", "boot-mode={} fold_verdicts={}", label, resolved == MODE_INTERACTIVE);
}

/// Host builds have no fw_cfg; the mode stays UNKNOWN (raw).
#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
pub fn detect() {}

/// True when the kernel should FOLD its boot markers into the verdict grid (interactive boot).
/// Proof and unknown both return `false` → raw markers, keeping `verify-uart` deterministic.
// NOTE: consumed by the diag verdict aggregator (next step); allow until then.
#[allow(dead_code)]
#[must_use]
pub fn fold_verdicts() -> bool {
    BOOT_MODE.load(Ordering::Relaxed) == MODE_INTERACTIVE
}
