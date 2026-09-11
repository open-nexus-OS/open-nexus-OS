// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Per-service slot constants (RFC-0093 §4, TASK-0324 P4) — the SAME values
//! `SERVICE_SPECS` carries, in a form a service can use in a `const` context. Declared here
//! ONCE, so init (which PINS a capability into the slot) and the service (which reads from
//! it) cannot drift apart. Split out of `specs.rs` under the module-size ratchet.
//!
//! Slots are migrated ONE CONSUMER PER PACKAGE (P4a-P4f); a service still absent from this
//! file reads `SlotPair::UNDECLARED` in its spec and keeps init's order-based transfer until
//! its package lands.
//!
//! OWNERS: @runtime
//! STATUS: Production (TASK-0324 P4)
//! API_STABILITY: Stable within the workspace
//! TEST_COVERAGE: `test_reject_slot_collision_per_service` + every QEMU lane

use crate::SlotPair;

/// The capability table of a SPAWNED APP CHILD (TASK-0324 P4e).
///
/// A per-app space, distinct from the service slots above: execd grants into it at
/// launch (`cap_transfer_to_slot`) and the app-host reads from it. Before P4e the same
/// numbers lived three times — execd's grant constants, the app-host's fixed constants
/// and `nexus-sdk-routes` — and the comments on each side said "the other side's fixed
/// constant", which is a contract only a reader can enforce.
pub mod app_child {
    use super::SlotPair;

    /// Windowd client route (present/attach; windowd answers on its own endpoint).
    pub const WINDOWD: SlotPair = SlotPair::new(5, 6);
    /// The app's `.nxir` payload VMO.
    pub const PAYLOAD_VMO: u32 = 7;
    /// ADR-0042 per-app event channel: the child's RECV half.
    pub const EVENTS_RECV: u32 = 8;
    /// Shared CAP_MOVE reply inbox for every `svc.*` call.
    pub const REPLY: SlotPair = SlotPair::new(10, 9);
    /// First per-service SEND slot; `nexus-sdk-routes` rows start here.
    pub const SVC_BASE: u32 = 11;
    /// SEND clone of the child's own event channel (it attaches this to windowd).
    pub const EVENTS_SEND: u32 = 14;
    /// Shared read-only glyph atlas VMO (RFC-0080).
    pub const ATLAS_VMO: u32 = 19;
    /// statefs route of the `demo.minidump` payload. Numerically the same slots as
    /// `PAYLOAD_VMO`/`EVENTS_RECV`, which is safe because those are only granted to
    /// app-host children and this pair only to the exit42 test image.
    pub const MINIDUMP_STATEFS: SlotPair = SlotPair::new(7, 8);
}

/// execd (TASK-0324 P4e-2).
///
/// execd's OWN table, distinct from the app-child table above: init grants into it at
/// wiring time, execd reads it. Before P4e-2 every number here came out of TRANSFER
/// ORDER — the arm carried "keep this block FIRST", "ARM END on purpose: transfers here
/// must never shift earlier positional slots" and a `probe_dump_cap_slots()` diagnostic
/// whose whole job was to name the drift after it had already happened.
pub mod execd {
    use super::SlotPair;

    /// execd's own server endpoint (abilitymgr's spawn requests arrive here).
    pub const SERVER: SlotPair = SlotPair::new(4, 3);
    /// The shared CAP_MOVE reply inbox for its outbound calls.
    pub const REPLY: SlotPair = SlotPair::new(6, 5);
    /// Crash-report appends (TASK-0049; fire-and-forget, the reply is not awaited).
    pub const LOGD: SlotPair = SlotPair::new(7, REPLY.recv);
    /// The windowd client route execd DELEGATES rather than uses: it never calls
    /// windowd itself, it clones both halves into every app child's
    /// [`super::app_child::WINDOWD`] before resume (ADR-0042).
    pub const WINDOWD: SlotPair = SlotPair::new(8, 9);
    /// `OP_GET_PAYLOAD`: the spawned app's `.nxir` payload VMO (TASK-0080D).
    pub const BUNDLEMGRD: SlotPair = SlotPair::new(10, REPLY.recv);
    /// recv-wake probe, ping leg: execd SENDs, the child RECVs. Two one-way endpoints
    /// because one shared queue would let execd's reply-wait steal its own ping.
    pub const PROBE_PING: SlotPair = SlotPair::new(11, 12);
    /// recv-wake probe, reply leg: the child SENDs, execd RECVs.
    pub const PROBE_REPLY: SlotPair = SlotPair::new(13, 14);
    /// `svc.ability.*` (launcher e2e).
    pub const ABILITYMGR: SlotPair = SlotPair::new(15, REPLY.recv);
    /// `svc.session.*` (DSL greeter login).
    pub const SESSIOND: SlotPair = SlotPair::new(16, REPLY.recv);
    /// `svc.settings.*` (settings app / Control Center).
    pub const SETTINGSD: SlotPair = SlotPair::new(17, REPLY.recv);
    /// `svc.time.*` / clock tick (RFC-0076).
    pub const TIMED: SlotPair = SlotPair::new(18, REPLY.recv);
    /// `svc.ime.osk` (RFC-0075 Phase 2) — a SEND clone of imed's OSK endpoint.
    pub const IMED_OSK: SlotPair = SlotPair::new(19, REPLY.recv);
    /// `svc.files.*` (filemanager role, RFC-0073).
    pub const VFSD: SlotPair = SlotPair::new(20, REPLY.recv);
    /// `svc.updates.*` (settings Updates page, TASK-0140).
    pub const UPDATED: SlotPair = SlotPair::new(21, REPLY.recv);
    /// Crash dumps: execd's own writer AND the pair it clones into `demo.minidump`
    /// children. A `SharedResponse` pair — execd's own wire use is nonce-matched v2.
    pub const STATEFSD: SlotPair = SlotPair::new(22, 23);
    /// The crash writer's attach-level gate (`crash.attach.*`, TASK-0051B).
    pub const POLICYD: SlotPair = SlotPair::new(24, REPLY.recv);
}

/// gpud (TASK-0324 P4c).
pub mod gpud {
    use super::SlotPair;

    /// gpud's own server endpoint (windowd presents here).
    pub const SERVER: SlotPair = SlotPair::new(4, 3);
}

/// hidrawd (TASK-0324 P4d).
pub mod hidrawd {
    use super::SlotPair;

    /// Normalized HID events to inputd (inputd answers on its own endpoint).
    pub const INPUTD: SlotPair = SlotPair::new(3, 4);
}

/// inputd (TASK-0324 P4b).
pub mod inputd {
    use super::SlotPair;

    /// inputd's own server endpoint.
    pub const SERVER: SlotPair = SlotPair::new(4, 3);
    /// Visible-state push to windowd (windowd answers on its own endpoint).
    pub const WINDOWD: SlotPair = SlotPair::new(5, 6);
    /// Key-forward leg to imed (RFC-0075).
    pub const IMED: SlotPair = SlotPair::new(7, 8);
    /// SEND to settingsd (`OP_WATCH` registration + reads).
    pub const SETTINGS_SEND: u32 = 0x20;
    /// Settings push channel: RECV half (event inbox).
    pub const WATCH_RECV: u32 = 0x21;
    /// Settings push channel: SEND half (moved with `OP_WATCH`).
    pub const WATCH_SEND: u32 = 0x22;
}

/// The capability table of execd's recv-wake probe child (TASK-0324 P4e-2).
///
/// Granted BEFORE `task_resume` (grants-before-resume discipline), so the child's very
/// first instruction can use them. The two numbers lived twice — execd's
/// `PROBE_CHILD_*_SLOT` and the probe binary's own consts, each commented as the other
/// side's contract.
pub mod recv_wake_probe {
    /// RECV half of the ping endpoint (execd holds [`super::execd::PROBE_PING`]`.send`).
    pub const PING_RECV: u32 = 5;
    /// SEND half of the reply endpoint (execd holds [`super::execd::PROBE_REPLY`]`.recv`).
    pub const REPLY_SEND: u32 = 6;
}

/// windowd (TASK-0324 P4a).
pub mod windowd {
    use super::SlotPair;

    /// windowd's own server endpoint (clients send here).
    pub const SERVER: SlotPair = SlotPair::new(4, 3);
    /// The shared CAP_MOVE reply inbox for its outbound calls.
    pub const REPLY: SlotPair = SlotPair::new(8, 7);
    /// Present/attach/cursor handoff to gpud (its own response endpoint).
    pub const GPUD: SlotPair = SlotPair::new(5, 6);
    /// Dynamic Apps menu (`OP_LIST_APPS`).
    pub const BUNDLEMGRD: SlotPair = SlotPair::new(9, REPLY.recv);
    /// Greeter/login relay.
    pub const SESSIOND: SlotPair = SlotPair::new(10, REPLY.recv);
    /// Theme GET/SET persistence.
    pub const SETTINGSD: SlotPair = SlotPair::new(11, REPLY.recv);
    /// `OP_LAUNCH` from the shell (abilitymgr answers on its own endpoint).
    pub const ABILITYMGR: SlotPair = SlotPair::new(12, 13);
    /// Focus relay `OP_SET_FOCUS`.
    pub const IMED: SlotPair = SlotPair::new(14, REPLY.recv);
    /// Settings push channel (RFC-0083): RECV half, drained per frame.
    pub const WATCH_RECV: u32 = 0x40;
    /// Settings push channel: SEND half, cloned per `OP_WATCH`.
    pub const WATCH_SEND: u32 = 0x41;
}
