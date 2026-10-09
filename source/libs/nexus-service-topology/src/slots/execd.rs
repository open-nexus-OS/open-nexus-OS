// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The app launcher's (`execd`) declared slots (TASK-0324 P4e-2) — execd's OWN table,
//! distinct from the app-child table ([`super::app_child`]): init grants into it at wiring time,
//! execd reads it. Before P4e-2 every number here came out of TRANSFER ORDER — the arm carried
//! "keep this block FIRST", "ARM END on purpose: transfers here must never shift earlier
//! positional slots" and a `probe_dump_cap_slots()` diagnostic whose whole job was to name the
//! drift after it had already happened. Split out of `slots/mod.rs` by the structure gate when
//! the screen-capture leg joined it (TASK-0068): every SDK service adds a row here.
//! OWNERS: @runtime
//! STATUS: Production
//! API_STABILITY: Stable within the workspace — the numbers are the contract between init and
//! execd

use super::SlotPair;

/// execd's own server endpoint (abilitymgr's spawn requests arrive here).
pub const SERVER: SlotPair = crate::SERVER_SLOTS;
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
/// Timer-notify endpoint (TASK-0054C P2-b): the recv-wake probe's FAIL witness — a
/// one-shot on a waitset beside the probe reply endpoint, never a recv deadline.
pub const TIMER: SlotPair = SlotPair::new(26, 25);
/// `svc.clipboard.*` (TASK-0067): cloned into app children holding `CLIPBOARD`.
pub const CLIPBOARDD: SlotPair = SlotPair::new(27, REPLY.recv);
/// `svc.screencap.*` (TASK-0068): cloned into app children holding `SCREENCAP`.
pub const SCREENCAPD: SlotPair = SlotPair::new(28, REPLY.recv);
