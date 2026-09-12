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

/// abilitymgr (TASK-0324 P4f-1a). The execd leg is a SharedResponse route (init hands over
/// execd's request AND response endpoint); it was declared `ReplyInbox` while provisioned as
/// SharedResponse by a special block in the generic arm.
pub mod abilitymgr {
    use super::SlotPair;

    /// abilitymgr's own server endpoint (windowd's `OP_LAUNCH` arrives here).
    pub const SERVER: SlotPair = crate::SERVER_SLOTS;
    /// Spawn route to execd — replies on execd's own response endpoint.
    pub const EXECD: SlotPair = SlotPair::new(5, 6);
    /// The shared CAP_MOVE reply inbox for its outbound calls.
    pub const REPLY: SlotPair = SlotPair::new(8, 7);
    /// Installed-app resolution.
    pub const BUNDLEMGRD: SlotPair = SlotPair::new(9, REPLY.recv);
    /// Launch gate: a session must be active.
    pub const SESSIOND: SlotPair = SlotPair::new(10, REPLY.recv);
}

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

/// bootctld (TASK-0324 P4f-1b). Fixed on purpose: init's boot-attempt handshake talks to
/// bootctld before the responder serves, so bootctld uses these slots directly and never
/// resolves them. Declaring them changes where the numbers come from, not that they are
/// fixed — its bespoke wiring function pinned the same literals and declared none of it.
pub mod bootctld {
    use super::SlotPair;

    /// bootctld's own server endpoint.
    pub const SERVER: SlotPair = crate::SERVER_SLOTS;
    /// The shared CAP_MOVE reply inbox (statefsd and policyd answer on it).
    pub const REPLY: SlotPair = SlotPair::new(6, 5);
    /// The boot record's statefs wire.
    pub const STATEFSD: SlotPair = SlotPair::new(7, REPLY.recv);
    /// `boot.target` / `boot.reset` delegated checks.
    pub const POLICYD: SlotPair = SlotPair::new(8, REPLY.recv);
}

/// bundlemgrd (TASK-0324 P4f-3). It verifies and serves the system volume from the core plane
/// on, i.e. it RUNS before init's wiring phase and allocates volume-window VMOs itself — so the
/// grants init makes later sit in the late-grant band. bundlemgrd resolves them by name; the old
/// order-based transfer put them wherever its own allocations had left room.
pub mod bundlemgrd {
    use super::SlotPair;

    /// bundlemgrd's own server endpoint (pinned in the core plane, before it runs).
    pub const SERVER: SlotPair = crate::SERVER_SLOTS;
    /// The shared CAP_MOVE reply inbox (a late grant).
    pub const REPLY: SlotPair = SlotPair::new(crate::LATE_GRANT_BASE + 1, crate::LATE_GRANT_BASE);
    /// Structured logs (a late grant).
    pub const LOGD: SlotPair = SlotPair::new(crate::LATE_GRANT_BASE + 2, REPLY.recv);
}

/// dsoftbusd (TASK-0324 P4f-4). Its netstackd route used to occupy 3/4 — the fleet's server
/// slots — while its own server landed wherever transfer order put it; both sides read this
/// declaration now, so dsoftbusd follows the fleet convention like every other service.
pub mod dsoftbusd {
    use super::SlotPair;

    /// dsoftbusd's own server endpoint.
    pub const SERVER: SlotPair = crate::SERVER_SLOTS;
    /// The shared CAP_MOVE reply inbox.
    pub const REPLY: SlotPair = SlotPair::new(6, 5);
    /// The netstackd facade. It answers every RPC on the caller's CAP_MOVE inbox and nowhere
    /// else, so the dedicated response endpoint init used to mint for this leg never carried
    /// a byte.
    pub const NETSTACKD: SlotPair = SlotPair::new(7, REPLY.recv);
    /// Service registry lookups.
    pub const SAMGRD: SlotPair = SlotPair::new(9, REPLY.recv);
    /// Bundle queries.
    pub const BUNDLEMGRD: SlotPair = SlotPair::new(0x0A, REPLY.recv);
    /// Remote packagefs read-only path (TASK-0016) — packagefsd's own response endpoint.
    pub const PACKAGEFSD: SlotPair = SlotPair::new(0x0B, 0x0C);
    /// Remote statefs proxy (TASK-0017) — statefsd's own response endpoint.
    pub const STATEFSD: SlotPair = SlotPair::new(0x0D, 0x0E);
    /// Structured logs.
    pub const LOGD: SlotPair = SlotPair::new(0x0F, REPLY.recv);
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
}

/// gpud (TASK-0324 P4c).
pub mod gpud {
    use super::SlotPair;

    /// gpud's own server endpoint (windowd presents here).
    pub const SERVER: SlotPair = crate::SERVER_SLOTS;
}

/// hidrawd (TASK-0324 P4d).
pub mod hidrawd {
    use super::SlotPair;

    /// Normalized HID events to inputd (inputd answers on its own endpoint).
    pub const INPUTD: SlotPair = SlotPair::new(3, 4);
}

/// imed (TASK-0324 P4f-1b).
pub mod imed {
    use super::SlotPair;

    /// imed's own server endpoint (inputd forwards keys, windowd relays focus).
    pub const SERVER: SlotPair = crate::SERVER_SLOTS;
    /// RECV half of the on-screen-keyboard endpoint; imed answers through `SERVER.send`.
    pub const OSK_RECV: u32 = 5;
    /// Commit/action pushes to windowd (windowd answers on its own endpoint).
    pub const WINDOWD: SlotPair = SlotPair::new(6, 7);
    /// Layout persistence (`input.keymap`): request SEND + the leg's private inbox RECV.
    pub const SETTINGSD: SlotPair = SlotPair::new(8, 9);
    /// SEND half of the settingsd leg's private inbox (moved with every request).
    pub const SETTINGSD_INBOX_SEND: u32 = 10;
    /// Ranking-blob persistence (TASK-0204): request SEND + the leg's private inbox RECV.
    pub const STATEFSD: SlotPair = SlotPair::new(0x0B, 0x0C);
    /// SEND half of the statefsd leg's private inbox.
    pub const STATEFSD_INBOX_SEND: u32 = 0x0D;
}

/// ingressd (TASK-0324 P4f-1a).
pub mod ingressd {
    use super::SlotPair;

    /// ingressd's own server endpoint (the selftest registers exposure intents here).
    pub const SERVER: SlotPair = crate::SERVER_SLOTS;
    /// The shared CAP_MOVE reply inbox (policyd and netstackd answer on it).
    pub const REPLY: SlotPair = SlotPair::new(6, 5);
    /// `net.expose` checks for the declared subject (RFC-0092).
    pub const POLICYD: SlotPair = SlotPair::new(7, REPLY.recv);
    /// The facade client leg: listen/accept/connect/relay.
    pub const NETSTACKD: SlotPair = SlotPair::new(8, REPLY.recv);
}

/// inputd (TASK-0324 P4b).
pub mod inputd {
    use super::SlotPair;

    /// inputd's own server endpoint.
    pub const SERVER: SlotPair = crate::SERVER_SLOTS;
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

/// keystored (TASK-0324 P4f-2). Its policyd leg used to land on 9 only because the optional
/// logd leg was transferred first; without logd it would have been 8 while keystored's policy
/// check asked slot 9 — the slot rngd's leg would then occupy. Declared, every leg is fixed.
pub mod keystored {
    use super::SlotPair;

    /// keystored's own server endpoint.
    pub const SERVER: SlotPair = crate::SERVER_SLOTS;
    /// The shared CAP_MOVE reply inbox.
    pub const REPLY: SlotPair = SlotPair::new(6, 5);
    /// The sealed key store.
    pub const STATEFSD: SlotPair = SlotPair::new(7, REPLY.recv);
    /// Structured logs (optional target).
    pub const LOGD: SlotPair = SlotPair::new(8, REPLY.recv);
    /// Delegated capability checks.
    pub const POLICYD: SlotPair = SlotPair::new(9, REPLY.recv);
    /// Entropy for key generation.
    pub const RNGD: SlotPair = SlotPair::new(0x0A, REPLY.recv);
}

/// logd (TASK-0324 P4f-1a).
pub mod logd {
    use super::SlotPair;

    /// logd's own server endpoint.
    pub const SERVER: SlotPair = crate::SERVER_SLOTS;
    /// The shared CAP_MOVE reply inbox.
    pub const REPLY: SlotPair = SlotPair::new(6, 5);
    /// Evidence spill (TASK-0049C).
    pub const STATEFSD: SlotPair = SlotPair::new(7, REPLY.recv);
}

/// metricsd (TASK-0324 P4f-4).
pub mod metricsd {
    use super::SlotPair;

    /// metricsd's own server endpoint.
    pub const SERVER: SlotPair = crate::SERVER_SLOTS;
    /// The shared CAP_MOVE reply inbox.
    pub const REPLY: SlotPair = SlotPair::new(6, 5);
    /// Retention writer.
    pub const STATEFSD: SlotPair = SlotPair::new(7, REPLY.recv);
    /// Snapshot/span export.
    pub const LOGD: SlotPair = SlotPair::new(8, REPLY.recv);
}

/// netstackd (TASK-0324 P4f-4). Its facade listened on 5/6 while the pair distributed at spawn
/// landed on 3/4, so init handed netstackd the SAME server pair twice (order at 3/4, pinned at
/// 5/6). Declared once, at the fleet convention, read by the facade and by init.
pub mod netstackd {
    use super::SlotPair;

    /// netstackd's own server endpoint (the facade).
    pub const SERVER: SlotPair = crate::SERVER_SLOTS;
    /// The seam's `@reply` pair.
    pub const REPLY: SlotPair = SlotPair::new(6, 5);
    /// RFC-0091 seam: connect/listen/bind evaluated at policyd.
    pub const POLICYD: SlotPair = SlotPair::new(7, REPLY.recv);
}

/// packagefsd (TASK-0324 P4f-1a).
pub mod packagefsd {
    use super::SlotPair;

    /// packagefsd's own server endpoint (vfsd resolves `pkg:/` here).
    pub const SERVER: SlotPair = crate::SERVER_SLOTS;
    /// The pre-minted CAP_MOVE reply inbox.
    pub const REPLY: SlotPair = SlotPair::new(6, 5);
    /// Slot and manifest queries.
    pub const BUNDLEMGRD: SlotPair = SlotPair::new(7, REPLY.recv);
}

/// pinched (TASK-0324 P4f-1a). A pure server; its respawn re-provisions exactly this pair.
pub mod pinched {
    use super::SlotPair;

    /// pinched's own server endpoint.
    pub const SERVER: SlotPair = crate::SERVER_SLOTS;
}

/// policyd (TASK-0324 P4f-2). policyd is resumed with the core plane, BEFORE init's wiring
/// phase, so everything init grants it later sits above what policyd allocates itself; its
/// check channels are pinned in the core plane, before it runs.
pub mod policyd {
    use super::SlotPair;

    /// policyd's own server endpoint (capability checks from the fleet).
    pub const SERVER: SlotPair = crate::SERVER_SLOTS;
    /// init's private route-check channel (policyd receives on 5, answers on 6).
    pub const ROUTE_CHECK: SlotPair = SlotPair::new(6, 5);
    /// init's private exec-check channel (policyd receives on 7, answers on 8).
    pub const EXEC_CHECK: SlotPair = SlotPair::new(8, 7);
    /// The shared CAP_MOVE reply inbox of its audit path.
    pub const REPLY: SlotPair = SlotPair::new(0x0A, 0x09);
    /// Audit records. A clone pair transferred in the core plane once landed here and
    /// silently displaced this leg — every audit record went to a dead slot (2026-09-08).
    pub const LOGD: SlotPair = SlotPair::new(0x0B, REPLY.recv);
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

/// rngd (TASK-0324 P4f-1a). Its policyd leg used to land on 7 or 8 depending on whether logd
/// was in the image (the logd transfer came first); declared, it is 8 either way.
pub mod rngd {
    use super::SlotPair;

    /// rngd's own server endpoint.
    pub const SERVER: SlotPair = crate::SERVER_SLOTS;
    /// The shared CAP_MOVE reply inbox.
    pub const REPLY: SlotPair = SlotPair::new(6, 5);
    /// Log sink (optional target).
    pub const LOGD: SlotPair = SlotPair::new(7, REPLY.recv);
    /// Delegated policy checks.
    pub const POLICYD: SlotPair = SlotPair::new(8, REPLY.recv);
}

/// samgrd (TASK-0324 P4f-1a).
pub mod samgrd {
    use super::SlotPair;

    /// samgrd's own server endpoint.
    pub const SERVER: SlotPair = crate::SERVER_SLOTS;
    /// The shared CAP_MOVE reply inbox.
    pub const REPLY: SlotPair = SlotPair::new(6, 5);
    /// Structured logs.
    pub const LOGD: SlotPair = SlotPair::new(7, REPLY.recv);
}

/// selftest-client, the proof harness (TASK-0324 P4f-5).
///
/// Before P4f-5 every number here was TRANSFER ORDER inside init's selftest arm: the harness
/// hardcoded eight of them in `route_with_retry`, the reply inbox in eight files and rngd in
/// three, and the arm's comments ("LAST in this arm so every earlier slot keeps its historical
/// number") were the only contract. Init now pins every leg here BEFORE the harness first runs,
/// and the harness checks init's routing answers against this table. The numbers are the
/// effective ones of the order-based layout, so the move is behaviour-neutral by construction.
pub mod selftest_client {
    use super::SlotPair;

    /// The shared CAP_MOVE reply inbox (probes clone its SEND half into every request).
    pub const REPLY: SlotPair = SlotPair::new(0x18, 0x17);
    /// vfsd (`SELFTEST: vfs …`).
    pub const VFSD: SlotPair = SlotPair::new(3, 4);
    /// packagefsd (`pkg:/` reads).
    pub const PACKAGEFSD: SlotPair = SlotPair::new(5, 6);
    /// policyd (allow/deny, ABI filter, audit probes).
    pub const POLICYD: SlotPair = SlotPair::new(7, 8);
    /// bundlemgrd (list, volume status, malformed frame).
    pub const BUNDLEMGRD: SlotPair = SlotPair::new(9, 0x0A);
    /// updated (OTA stage/switch/commit).
    pub const UPDATED: SlotPair = SlotPair::new(0x0B, 0x0C);
    /// samgrd (registry register/lookup).
    pub const SAMGRD: SlotPair = SlotPair::new(0x0D, 0x0E);
    /// execd (spawn, crash, restart probes).
    pub const EXECD: SlotPair = SlotPair::new(0x0F, 0x10);
    /// keystored (device key, signing).
    pub const KEYSTORED: SlotPair = SlotPair::new(0x11, 0x12);
    /// statefsd (CRUD, persistence, encryption).
    pub const STATEFSD: SlotPair = SlotPair::new(0x13, 0x14);
    /// logd (append/query, the `nexus-log` sink).
    pub const LOGD: SlotPair = SlotPair::new(0x15, 0x16);
    /// inputd (answers on the reply inbox).
    pub const INPUTD: SlotPair = SlotPair::new(0x19, REPLY.recv);
    /// netstackd's facade — it answers every RPC on the caller's CAP_MOVE cap only.
    pub const NETSTACKD: SlotPair = SlotPair::new(0x1A, REPLY.recv);
    /// dsoftbusd (local IPC probe).
    pub const DSOFTBUSD: SlotPair = SlotPair::new(0x1C, 0x1D);
    /// rngd (entropy probe).
    pub const RNGD: SlotPair = SlotPair::new(0x1E, 0x1F);
    /// timed (coalescing probe).
    pub const TIMED: SlotPair = SlotPair::new(0x20, 0x23);
    /// metricsd (counters, retention, spans).
    pub const METRICSD: SlotPair = SlotPair::new(0x21, 0x22);
    /// pinched (compute broker; re-resolved by name after a restart, ADR-0057).
    pub const PINCHED: SlotPair = SlotPair::new(0x24, 0x25);
    /// settingsd (watch probe).
    pub const SETTINGSD: SlotPair = SlotPair::new(0x26, 0x27);
    /// ingressd — the gateway answers on the caller's CAP_MOVE cap.
    pub const INGRESSD: SlotPair = SlotPair::new(0x28, REPLY.recv);
    /// imed (the IME authority's negative probe).
    pub const IMED: SlotPair = SlotPair::new(0x2A, 0x2B);
    /// imed's on-screen-keyboard endpoint — a clone made for the harness; replies ride the
    /// probe's own minted channel.
    pub const IMED_OSK: SlotPair = SlotPair::new(0x2C, REPLY.recv);
    /// bootctld (reset/target lane; a successful reset never answers).
    pub const BOOTCTLD: SlotPair = SlotPair::new(0x2D, REPLY.recv);
    /// virtioblkd's request endpoint for the deny probes — NOT a block-plane grant: the
    /// probes prove that a sender without one is refused.
    pub const VIRTIOBLKD: SlotPair = SlotPair::new(0x2E, REPLY.recv);
    /// The QEMU firmware-config window the harness reads its boot profile from.
    pub const FW_CFG: u32 = 0x31;
}

/// sessiond (TASK-0324 P4f-1a). A pure server.
pub mod sessiond {
    use super::SlotPair;

    /// sessiond's own server endpoint.
    pub const SERVER: SlotPair = crate::SERVER_SLOTS;
}

/// settingsd (TASK-0324 P4f-1a).
pub mod settingsd {
    use super::SlotPair;

    /// settingsd's own server endpoint.
    pub const SERVER: SlotPair = crate::SERVER_SLOTS;
    /// The shared CAP_MOVE reply inbox.
    pub const REPLY: SlotPair = SlotPair::new(6, 5);
    /// Preference persistence.
    pub const STATEFSD: SlotPair = SlotPair::new(7, REPLY.recv);
}

/// statefsd (TASK-0324 P4f-1a). The policy leg is the one `nexus_ipc::policyd::check_cap_on`
/// used to receive as the literal arguments `(0x07, 0x06, 0x05)`.
pub mod statefsd {
    use super::SlotPair;

    /// statefsd's own server endpoint.
    pub const SERVER: SlotPair = crate::SERVER_SLOTS;
    /// The shared CAP_MOVE reply inbox (cap check, ABI seam and logd append share it).
    pub const REPLY: SlotPair = SlotPair::new(6, 5);
    /// Capability checks and the RFC-0091 argument seam.
    pub const POLICYD: SlotPair = SlotPair::new(7, REPLY.recv);
    /// The audit trail to logd (TASK-0324 P4f-6). Every audit record statefsd emitted was sent
    /// to slot 8, which init never provisioned — the leg existed only in statefsd's code. statefsd
    /// runs from wave 1 on, before wiring, so this late grant sits in the late-grant band.
    pub const LOGD: SlotPair = SlotPair::new(crate::LATE_GRANT_BASE, REPLY.recv);
}

/// timed (TASK-0324 P4f-1a). A pure server.
pub mod timed {
    use super::SlotPair;

    /// timed's own server endpoint.
    pub const SERVER: SlotPair = crate::SERVER_SLOTS;
}

/// updated (TASK-0324 P4f-3). Its bundlemgrd leg is a reply-inbox route: the dedicated
/// response endpoint init used to mint for it at slot 6 was never read ("unused, we use reply
/// inbox"), so it is no longer granted.
pub mod updated {
    use super::SlotPair;

    /// updated's own server endpoint.
    pub const SERVER: SlotPair = crate::SERVER_SLOTS;
    /// Active-slot publication.
    pub const BUNDLEMGRD: SlotPair = SlotPair::new(5, REPLY.recv);
    /// Signature verification — replies on keystored's own response endpoint.
    pub const KEYSTORED: SlotPair = SlotPair::new(7, 8);
    /// Persistence.
    pub const STATEFSD: SlotPair = SlotPair::new(9, REPLY.recv);
    /// The shared CAP_MOVE reply inbox.
    pub const REPLY: SlotPair = SlotPair::new(0x0B, 0x0A);
    /// Staging-source splice reads (replies ride the VMO header).
    pub const VFSD: SlotPair = SlotPair::new(0x0C, REPLY.recv);
    /// Slot mutations delegate to bootctld.
    pub const BOOTCTLD: SlotPair = SlotPair::new(0x0D, REPLY.recv);
    /// The `updates.manage` gate.
    pub const POLICYD: SlotPair = SlotPair::new(0x0E, REPLY.recv);
    /// Structured logs.
    pub const LOGD: SlotPair = SlotPair::new(0x0F, REPLY.recv);
}

/// vfsd (TASK-0324 P4f-1a).
pub mod vfsd {
    use super::SlotPair;

    /// vfsd's own server endpoint.
    pub const SERVER: SlotPair = crate::SERVER_SLOTS;
    /// `pkg:/` resolution — replies on packagefsd's own response endpoint.
    pub const PACKAGEFSD: SlotPair = SlotPair::new(5, 6);
}

/// virtioblkd (TASK-0324 P4f-1b).
pub mod virtioblkd {
    use super::SlotPair;

    /// virtioblkd's own server endpoint (block-plane clients send here). It holds NO reply
    /// inbox: the driver makes no outbound call, and the inbox init used to provision was
    /// never read — declaring it at 5/6 collided with the driver's own virtqueue VMOs, which it
    /// allocates at the lowest free slots because it runs before init wires it.
    pub const SERVER: SlotPair = crate::SERVER_SLOTS;
    /// RECV half of the dedicated IRQ-completion notify endpoint. It used to sit at 0xF1 —
    /// the same number as every block-plane client's reply RECV, so a reader could not tell
    /// the driver's IRQ slot from a client's reply slot; 0xF3 extends the block-plane family
    /// instead of aliasing into it (both sides read this constant, so the move is safe by
    /// construction).
    pub const IRQ_NOTIFY: u32 = 0xF3;
}

/// windowd (TASK-0324 P4a).
pub mod windowd {
    use super::SlotPair;

    /// windowd's own server endpoint (clients send here).
    pub const SERVER: SlotPair = crate::SERVER_SLOTS;
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
