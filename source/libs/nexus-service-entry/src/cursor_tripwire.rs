// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the bump cursor's monotonicity tripwire, split out of `lib.rs`
//! under the structure ratchet (TASK-0077C P2b).
//!
//! A bump cursor only ever moves forward. One that moves BACKWARDS means the
//! allocator's own state was overwritten — a stack cliff into `.bss`, say —
//! and that is the root of overlapping allocations and of corruption that
//! reads as impossible. Checked on every allocation, reported once, loudly.
//! It lives beside the allocator rather than inside it because it is a
//! DIAGNOSTIC: it observes the allocator and never decides anything for it.
//!
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: fires only on corruption; exercised by every OS boot not firing

#![cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::debug_write::{debug_write_byte, debug_write_bytes, debug_write_hex, debug_write_str};
use crate::os::service_name;

/// Task #14 tripwire: the bump cursor is MONOTONE by construction. A
/// cursor that moves backwards means the allocator state was overwritten
/// (e.g. a stack cliff into .bss) — the root of overlapping allocations
/// and "impossible" data corruption. Checked on every alloc; violation
/// is reported once, loudly.
static LAST_CURSOR: AtomicUsize = AtomicUsize::new(0);
static CURSOR_REGRESSED: AtomicBool = AtomicBool::new(false);

pub(crate) fn check_cursor_monotone(cur_after: usize) {
    let prev = LAST_CURSOR.swap(cur_after, Ordering::AcqRel);
    if cur_after < prev && !CURSOR_REGRESSED.swap(true, Ordering::AcqRel) {
        debug_write_bytes(b"!alloc-cursor-regressed svc=");
        debug_write_str(service_name());
        debug_write_bytes(b" prev=0x");
        debug_write_hex(prev);
        debug_write_bytes(b" now=0x");
        debug_write_hex(cur_after);
        debug_write_byte(b'\n');
    }
}
