// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Minimal cross-phase context (`PhaseCtx`) for `os_lite::run()`.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Indirect — every QEMU marker that depends on cross-phase
//!   state (`reply_send_slot`, `reply_recv_slot`, `updated_pending`,
//!   `local_ip`, `os2vm`) covers `PhaseCtx` by construction.
//!
//! Holds only state read by ≥ 2 phases or directly observable in the QEMU
//! marker ladder. Service handles are intentionally NOT cached here in Phase 2
//! because the existing `pub fn run()` body already re-resolves
//! logd/policyd/bundlemgrd per-phase via `route_with_retry`; promoting them
//! would introduce risk without collapsing duplication. Later phases (P3+)
//! may extend this struct.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md
//!
//! Concurrency model (Send/Sync intent, TASK-0023B Cut P3-04):
//!   `PhaseCtx` is owned by a single HART, single task. The OS path is the
//!   `selftest-client` payload spawned by init-lite as one userspace task on a
//!   single hart; phases run sequentially via `os_lite::run()` and pass
//!   `&mut PhaseCtx` linearly. There is no shared-state concurrency. The
//!   struct intentionally does NOT carry `Send` / `Sync` marker traits or
//!   interior mutability primitives: introducing them later without first
//!   changing the runtime model would mask, not reveal, a real concurrency
//!   bug. Phase 4 may revisit if a multi-task probe is added.

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec::Vec;

/// Cross-phase state for `os_lite::run()`.
///
/// Allowed (per RFC-0038 Phase-2 minimality rule): cross-phase data + state that
/// directly determines the marker ladder. Forbidden: phase-local timing windows,
/// retry counters scoped to one phase, transient buffers.
pub(crate) struct PhaseCtx {
    // TODO(TASK-0023B Phase 4): wrap `reply_send_slot`/`reply_recv_slot` (and
    // the parallel `(send|recv)_slot: u32` fields under `services/`,
    // `updated/`, `ipc/`) in a `Slot(u32)` newtype to prevent
    // send/recv-direction confusion. Deferred from Cut P3-04 because the
    // change is non-mechanical (≥ 16 call sites across 8 files) and belongs
    // with the marker-manifest work in Phase 4.
    /// Send half of the deterministic @reply slot pair distributed by init-lite.
    pub(crate) reply_send_slot: u32,
    /// Receive half of the deterministic @reply slot pair distributed by init-lite.
    pub(crate) reply_recv_slot: u32,
    /// Pending out-of-order replies observed while pumping the shared `updated`
    /// inbox (RFC-0019 nonce correlation). Crosses routing → ota.
    pub(crate) updated_pending: VecDeque<Vec<u8>>,
    /// Local IPv4 (resolved during the `net` phase, consumed by `remote`).
    pub(crate) local_ip: Option<[u8; 4]>,
    /// True iff this is Node A in the 2-VM os2vm harness mode.
    pub(crate) os2vm: bool,
}

impl PhaseCtx {
    /// Build the initial cross-phase context. MUST be silent: emits no UART
    /// markers and performs no service routing. Returns `Err(())` only if a
    /// future infallible step becomes fallible.
    pub(crate) fn bootstrap() -> Result<Self, ()> {
        // @reply slots are deterministically distributed by init-lite to
        // selftest-client. The routing control-plane now supports a nonce-
        // correlated extension, but we still avoid routing to "@reply" here to
        // keep the proof independent from ctrl-plane behavior.
        const REPLY_RECV_SLOT: u32 = 0x17;
        const REPLY_SEND_SLOT: u32 = 0x18;
        Ok(Self {
            reply_send_slot: REPLY_SEND_SLOT,
            reply_recv_slot: REPLY_RECV_SLOT,
            updated_pending: VecDeque::new(),
            local_ip: None,
            os2vm: false,
        })
    }
}
