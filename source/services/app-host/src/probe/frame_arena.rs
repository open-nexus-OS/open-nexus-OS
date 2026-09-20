// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: what the frame phase costs, reported from inside it (ADR-0065,
//! TASK-0077C P2). Split out of `scroll.rs` under the structure ratchet — the
//! numbers belong to the frame arena, not to scrolling.
//!
//! The app-host rebuilds a frame on every structural interaction and throws it
//! away: 226 560 B per layout call, 0 B of live drift over 100 dispatches. On a
//! never-freeing bump that leaks in full, which is a ceiling of 50-85
//! interactions. `relayout_retained` now runs inside a generation; this is how
//! a boot log says whether that worked.
//!
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the `apphost: frame arena` / `heap steady` markers in the visible lane
//! ADR: docs/adr/0065-app-host-frame-phase-allocates-from-a-generation-arena.md

use super::*;

impl super::DslApp {
    /// The frame-arena numbers, every `ARENA_SAMPLE` layouts (ADR-0065).
    ///
    /// Two lines, and only one of them is a claim. `frame arena` reports what
    /// is: how far the base heap has advanced, the arena's high-water mark and
    /// whether a frame ever spilled out of it. `heap steady` is printed ONLY
    /// when the base heap did not move between two samples — the property this
    /// whole task exists to produce. A marker that said "steady" on every
    /// sample would be the fake-green this tree deletes on sight.
    ///
    /// A SPILL is the interesting failure: the arena was too small, the
    /// remainder went to the never-freeing base heap, and the leak is back.
    /// The lane asserts `spill=0`.
    #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
    pub(super) fn report_frame_arena(&mut self) {
        use core::sync::atomic::{AtomicUsize, Ordering};
        /// Layouts between samples. A whole visible boot produces about 15
        /// layouts (measured), so a wide interval would print once and show no
        /// trend at all — 4 gives four data points and the slope between them.
        const ARENA_SAMPLE: usize = 4;
        static LAYOUTS: AtomicUsize = AtomicUsize::new(0);
        static LAST_BASE: AtomicUsize = AtomicUsize::new(usize::MAX);

        // Sample the FIRST layout too: a boot short enough to produce fewer
        // than `ARENA_SAMPLE` layouts would otherwise print nothing at all,
        // and a proof that can be absent is not a proof.
        let n = LAYOUTS.fetch_add(1, Ordering::Relaxed) + 1;
        if n != 1 && n % ARENA_SAMPLE != 0 {
            return;
        }
        let (start, current, _) = nexus_service_entry::os::heap_cursor();
        let base = current.saturating_sub(start);
        let (peak, spilled, size) = nexus_service_entry::os::arena_stats();
        let mut m = alloc::string::String::new();
        let _ = core::fmt::write(
            &mut m,
            format_args!(
                "apphost: frame arena (layouts={n} base={base} peak={peak} of={size} spill={})",
                usize::from(spilled)
            ),
        );
        raw_marker(&m);
        let previous = LAST_BASE.swap(base, Ordering::Relaxed);
        if previous == base {
            let mut m = alloc::string::String::new();
            let _ = core::fmt::write(
                &mut m,
                format_args!("apphost: heap steady (layouts={n} base={base} peak={peak})"),
            );
            raw_marker(&m);
        }
    }

    /// Host build: there is no service allocator to report on.
    #[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
    pub(super) fn report_frame_arena(&mut self) {}
}
