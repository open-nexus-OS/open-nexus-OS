// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Kernel-side boot-mode resolution from `/chosen` (RFC-0098 C2, TASK-0245 P3). The
//! kernel emits its own boot markers (`[INFO sched]`, `KSELFTEST: …`, address-space/exec traces)
//! BEFORE userspace `init` runs, so it cannot be told the boot mode by anyone — it reads it from
//! the tree the previous stage handed over: `nexus,boot-mode` (`proof` | `interactive`, the
//! lane's `selftest-mode` knob re-expressed by nxboot from fw_cfg on QEMU; absent on the board
//! and on a direct-kernel boot). The kernel reads no fw_cfg and maps no fw_cfg window: every
//! lane knob arrives through the one hardware truth. The display-mode request it used to relay
//! (syscall 50) is gpud's to read since RFC-0098 C7 — the kernel holds no display policy.
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

/// Resolve the boot mode from `/chosen/nexus,boot-mode` ONCE, after the kernel address space
/// is active (the tree is identity-mapped read-only there). An absent or unparsable value
/// leaves the default (raw markers).
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
}

/// Host builds carry no tree; the mode stays UNKNOWN (raw).
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
}
