// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: how often an app-host proof line may fire. Anything a user's typing can trigger —
//! a keystroke's commit, the live search it re-runs, the frame it presents — proves its path
//! ONCE per process: a line per keystroke would write typing rhythm and length into the boot
//! log (the keystroke privacy rule, 2026-10-08). Counts that only matter when they grow log on
//! a new high-water mark. Pointer-only lines (a tap's trace) stay bounded per process and are
//! silent in the keyboard overlay, where a tap IS a keystroke.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// A proof line that fires once per process.
pub(crate) struct OnceLine(AtomicBool);

impl OnceLine {
    pub(crate) const fn new() -> Self {
        Self(AtomicBool::new(false))
    }

    /// `true` exactly once: on the first call.
    pub(crate) fn claim(&self) -> bool {
        !self.0.swap(true, Ordering::Relaxed)
    }
}

/// A count that logs only on a new high: growth stays visible, a steady or shrinking count
/// (each keystroke's re-render) stays silent.
pub(crate) struct HighWater(AtomicUsize);

impl HighWater {
    pub(crate) const fn new() -> Self {
        Self(AtomicUsize::new(0))
    }

    /// `true` when `n` exceeds every value seen before.
    pub(crate) fn raise(&self, n: usize) -> bool {
        self.0.fetch_max(n, Ordering::Relaxed) < n
    }
}

// The process-wide lines (OS-only users; the types above are what the host tests reach).

/// The first commit applied to a focused field.
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
pub(crate) static TEXT_COMMIT: OnceLine = OnceLine::new();
/// The first copy and the first paste out of / into a field.
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
pub(crate) static TEXT_COPY: OnceLine = OnceLine::new();
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
pub(crate) static TEXT_PASTE: OnceLine = OnceLine::new();
/// The first registry listing (the live search re-lists on every keystroke).
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
pub(crate) static ENUMERATE: OnceLine = OnceLine::new();
/// The first full interactive present (typing presents too).
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
pub(crate) static INTERACTIVE_PRESENT: OnceLine = OnceLine::new();
/// The resident text-run count of a scroll window (the store-window bound).
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
pub(crate) static SCROLL_TEXTS: HighWater = HighWater::new();

/// A proof line written straight to the UART — it bypasses the verdict folding the process
/// arms for every other line, so the service chain stays visible for boot verification.
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
pub(crate) fn raw_marker(line: &str) {
    let mut buf = [0u8; 96];
    let bytes = line.as_bytes();
    let n = bytes.len().min(buf.len() - 1);
    buf[..n].copy_from_slice(&bytes[..n]);
    buf[n] = b'\n';
    let _ = nexus_abi::debug_write(&buf[..n + 1]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_once_line_claims_exactly_once() {
        let line = OnceLine::new();
        assert!(line.claim());
        assert!(!(0..100).any(|_| line.claim()), "never again in this process");
    }

    #[test]
    fn a_high_water_mark_logs_only_growth() {
        let texts = HighWater::new();
        assert!(texts.raise(14), "the first value is a new high");
        assert!(!texts.raise(14), "a steady count is silent");
        assert!(!texts.raise(3), "a smaller count is silent");
        assert!(texts.raise(15), "growth is visible");
        assert!(!texts.raise(0));
    }

    #[test]
    fn test_reject_zero_as_a_high_water_mark() {
        assert!(!HighWater::new().raise(0), "nothing resident is nothing to report");
    }
}
