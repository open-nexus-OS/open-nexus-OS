// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Kernel-side boot-mode and display-mode resolution from `/chosen`
//! (RFC-0098 C2, TASK-0245 P3). The kernel emits its own boot markers
//! (`[INFO sched]`, `KSELFTEST: …`, address-space/exec traces) BEFORE userspace
//! `init` runs, so it cannot be told the boot mode by anyone — it reads it from
//! the tree the previous stage handed over: `nexus,boot-mode` (`proof` |
//! `interactive`, the lane's `selftest-mode` knob re-expressed by nxboot from
//! fw_cfg on QEMU; absent on the board and on a direct-kernel boot) and
//! `nexus,display-mode` (a `"<w>x<h>"` REQUEST, RFC-0074 — the authority moves
//! to gpud in RFC-0098 Phase 5). The kernel reads no fw_cfg and maps no fw_cfg
//! window: every lane knob arrives through the one hardware truth.
//!
//!   Purpose: gate whether the kernel FOLDS its boot markers into the verdict grid
//!   (interactive `just start`) or emits them RAW (proof `just test-os`, where
//!   `verify-uart` greps the individual `KSELFTEST:` markers). The default on ANY
//!   absent or unparsable value is PROOF/raw, so a missing knob can never silently
//!   break the proof harness.
//!
//! ALLOC-FREE by construction: the tree is parsed in place by `nexus-fdt`.
//!
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable

#![allow(clippy::missing_docs_in_private_items)]

use core::sync::atomic::{AtomicU8, Ordering};

const MODE_UNKNOWN: u8 = 0;
const MODE_PROOF: u8 = 1;
const MODE_INTERACTIVE: u8 = 2;

/// Resolved boot mode (set once by [`detect`]). Defaults to UNKNOWN, which folds like PROOF
/// (raw markers) so verify-uart is never disturbed by an absent knob.
static BOOT_MODE: AtomicU8 = AtomicU8::new(MODE_UNKNOWN);

/// Resolved display mode packed as `w | (h << 16)` (set once by [`detect`]).
/// `0` = unknown/absent → consumers fall back to their fixed layout maximum.
static DISPLAY_MODE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// Resolve boot mode + display mode from `/chosen/nexus,*` ONCE, after the kernel
/// address space is active (the tree is identity-mapped read-only there). Absent
/// or unparsable values leave the defaults (raw markers, layout maximum).
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub fn detect() {
    let Some(chosen) = crate::boot_fdt::bytes()
        .and_then(|b| nexus_fdt::Fdt::new(b).ok())
        .and_then(|f| f.chosen().ok())
    else {
        return;
    };
    if let Some(mode) = chosen.nexus_str("boot-mode") {
        let resolved = resolve_mode(mode.as_bytes());
        BOOT_MODE.store(resolved, Ordering::Relaxed);
        let label: &str = match resolved {
            MODE_PROOF => "proof",
            MODE_INTERACTIVE => "interactive",
            _ => "unknown",
        };
        // One-time boot diagnostic so the read is verifiable. Single atomic line.
        log_info!(target: "boot", "boot-mode={} fold_verdicts={}", label, resolved == MODE_INTERACTIVE);
    }
    if let Some(packed) = chosen.nexus_str("display-mode").and_then(|s| parse_wxh(s.as_bytes())) {
        DISPLAY_MODE.store(packed, Ordering::Relaxed);
        log_info!(target: "boot", "display-mode={}x{}", packed & 0xFFFF, packed >> 16);
    }
}

/// Host builds carry no tree; the modes stay UNKNOWN (raw).
#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
pub fn detect() {}

/// `proof` | `interactive…` → the mode byte; anything else is UNKNOWN (= raw).
fn resolve_mode(value: &[u8]) -> u8 {
    let len = value
        .iter()
        .position(|&c| c == 0 || c == b'\n' || c == b'\r' || c == b' ')
        .unwrap_or(value.len());
    let mode = &value[..len];
    if mode == b"proof" {
        MODE_PROOF
    } else if mode.starts_with(b"interactive") {
        MODE_INTERACTIVE
    } else {
        MODE_UNKNOWN
    }
}

/// Parse an ASCII `"<w>x<h>"` value into `w | (h << 16)`. Pure + bounded
/// (host-testable). Returns `None` on malformed / zero / oversized dimensions.
#[must_use]
pub fn parse_wxh(buf: &[u8]) -> Option<u32> {
    // Bound each dimension to a sane display ceiling; the compositor clamps to
    // its own layout max separately. Rejects garbage so a bad value never sizes
    // the scanout to a degenerate value.
    const MAX_DIM: u32 = 8192;
    let end = buf
        .iter()
        .position(|&c| c == 0 || c == b'\n' || c == b'\r' || c == b' ')
        .unwrap_or(buf.len());
    let text = &buf[..end];
    let sep = text.iter().position(|&c| c == b'x' || c == b'X')?;
    let w = parse_dim(&text[..sep], MAX_DIM)?;
    let h = parse_dim(&text[sep + 1..], MAX_DIM)?;
    Some(w | (h << 16))
}

fn parse_dim(bytes: &[u8], max: u32) -> Option<u32> {
    if bytes.is_empty() || bytes.len() > 5 {
        return None;
    }
    let mut v: u32 = 0;
    for &c in bytes {
        if !c.is_ascii_digit() {
            return None;
        }
        v = v.checked_mul(10)?.checked_add(u32::from(c - b'0'))?;
    }
    if v == 0 || v > max {
        return None;
    }
    Some(v)
}

/// The requested display mode packed as `w | (h << 16)`, or `0` when unknown/absent/host.
#[must_use]
pub fn display_mode() -> u32 {
    DISPLAY_MODE.load(Ordering::Relaxed)
}

/// True when the kernel should FOLD its boot markers into the verdict grid (interactive boot).
/// Proof and unknown both return `false` → raw markers, keeping `verify-uart` deterministic.
#[must_use]
pub fn fold_verdicts() -> bool {
    BOOT_MODE.load(Ordering::Relaxed) == MODE_INTERACTIVE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_words_resolve_and_garbage_stays_raw() {
        assert_eq!(resolve_mode(b"proof"), MODE_PROOF);
        assert_eq!(resolve_mode(b"proof\n"), MODE_PROOF);
        assert_eq!(resolve_mode(b"interactive"), MODE_INTERACTIVE);
        assert_eq!(resolve_mode(b"interactive-desktop"), MODE_INTERACTIVE);
        assert_eq!(resolve_mode(b""), MODE_UNKNOWN);
        assert_eq!(resolve_mode(b"PROOF"), MODE_UNKNOWN);
    }

    #[test]
    fn wxh_parses_and_rejects() {
        assert_eq!(parse_wxh(b"1280x800"), Some(1280 | (800 << 16)));
        assert_eq!(parse_wxh(b"1920X1080\0"), Some(1920 | (1080 << 16)));
        assert_eq!(parse_wxh(b"0x800"), None);
        assert_eq!(parse_wxh(b"9000x800"), None);
        assert_eq!(parse_wxh(b"1280"), None);
        assert_eq!(parse_wxh(b"12a0x800"), None);
    }
}
