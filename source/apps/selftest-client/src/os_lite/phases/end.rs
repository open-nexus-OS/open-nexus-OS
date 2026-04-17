// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Phase 12 of 12 — end (`SELFTEST: end` marker emission + cooperative
//!   idle loop; never returns).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — terminator marker.
//!
//! Extracted in Cut P2-13 of TASK-0023B. Marker order and marker strings are
//! byte-identical to the pre-cut body. Returns `!` because the cooperative
//! idle loop never exits; this phase is always the last call in
//! `os_lite::run()`.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use nexus_abi::yield_;

use crate::markers::emit_line;
use crate::os_lite::context::PhaseCtx;

pub(crate) fn run(_ctx: &mut PhaseCtx) -> ! {
    emit_line("SELFTEST: end");

    // Stay alive (cooperative).
    loop {
        let _ = yield_();
    }
}
