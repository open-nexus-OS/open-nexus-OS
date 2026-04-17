// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Phase 9 of 12 — vfs (cross-process VFS probe over kernel IPC v1
//!   via `vfs::verify_vfs()`; granular success markers emitted from inside
//!   `verify_vfs`, FAIL marker emitted at this layer).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — VFS slice.
//!
//! Extracted in Cut P2-10 of TASK-0023B. Marker order and marker strings are
//! byte-identical to the pre-cut body.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use crate::markers::emit_line;
use crate::os_lite::context::PhaseCtx;
use crate::os_lite::vfs;

pub(crate) fn run(_ctx: &mut PhaseCtx) -> core::result::Result<(), ()> {
    if vfs::verify_vfs().is_err() {
        emit_line("SELFTEST: vfs FAIL");
    }
    Ok(())
}
