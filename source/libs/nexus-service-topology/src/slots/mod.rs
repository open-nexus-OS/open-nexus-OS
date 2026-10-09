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
    /// Timer-notify endpoint (TASK-0324 P7-d, minted by execd): RECV half, a waitset member
    /// next to the event channel — the app's clock (minute boundary) without a recv timeout.
    pub const TIMER_RECV: u32 = 20;
    /// Timer-notify endpoint: SEND half the app's kernel timer cap is bound to.
    pub const TIMER_SEND: u32 = 21;
    /// The pair as one value (`nexus_ipc::timer::NotifyTimer::bind`).
    pub const TIMER: SlotPair = SlotPair::new(TIMER_SEND, TIMER_RECV);
    /// statefs route of the `demo.minidump` payload. Numerically the same slots as
    /// `PAYLOAD_VMO`/`EVENTS_RECV`, which is safe because those are only granted to
    /// app-host children and this pair only to the exit42 test image. `.recv` is the payload's
    /// PRIVATE reply endpoint since TASK-0054C P2-f — it used to be a RECV clone of statefsd's
    /// own response queue, which is the sharing that kept statefsd answering cap-less senders.
    pub const MINIDUMP_STATEFS: SlotPair = SlotPair::new(7, 8);
    /// The SEND half of that private endpoint. The payload MOVES this cap with its single PUT
    /// (the header is built at compile time — the generated child runs no `cap_clone`), so
    /// statefsd answers on slot 8 and nowhere else.
    pub const MINIDUMP_REPLY_SEND: u32 = 9;
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
    /// Counters (fire-and-forget; a late grant). TASK-0324 P7-b: this used to be a RUNTIME
    /// route ask (`new_for("metricsd")`) from inside a request handler — with init blocked in
    /// its own synchronous query to bundlemgrd and no timeout on the ask, that is a deadlock.
    /// A route is declared and pinned, never asked for while serving.
    pub const METRICSD: SlotPair = SlotPair::new(crate::LATE_GRANT_BASE + 3, REPLY.recv);
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
    /// Remote packagefs read-only path (TASK-0016). Answers on dsoftbusd's CAP_MOVE reply
    /// inbox since TASK-0033 P2 — packagefsd replies only to a sender that moved a reply cap,
    /// so its response endpoint has no readers left to take the wrong answer.
    pub const PACKAGEFSD: SlotPair = SlotPair::new(0x0B, REPLY.recv);
    /// Remote statefs proxy (TASK-0017). Answers on dsoftbusd's CAP_MOVE reply inbox since
    /// TASK-0054C P2-f — statefsd replies only to a sender that moved a reply cap.
    pub const STATEFSD: SlotPair = SlotPair::new(0x0D, REPLY.recv);
    /// Structured logs.
    pub const LOGD: SlotPair = SlotPair::new(0x0F, REPLY.recv);
}

/// execd (TASK-0324 P4e-2; its table is `slots/execd.rs`).
pub mod execd;

/// gpud (TASK-0324 P4c).
pub mod gpud {
    use super::SlotPair;

    /// gpud's own server endpoint (windowd presents here).
    pub const SERVER: SlotPair = crate::SERVER_SLOTS;
    /// Timer-notify endpoint (TASK-0324 P7-d): RECV half, a waitset member — the frame clock
    /// of the splash/build-up phases (a synthetic vblank on a device without one).
    pub const TIMER_RECV: u32 = 0x20;
    /// Timer-notify endpoint: SEND half the kernel timer cap is bound to.
    pub const TIMER_SEND: u32 = 0x21;
    /// The pair as one value (`nexus_ipc::timer::NotifyTimer::bind`).
    pub const TIMER: SlotPair = SlotPair::new(TIMER_SEND, TIMER_RECV);
    /// Device-watchdog endpoint (TASK-0054C P2-b): the GPU ring-buffer wait's bound — a
    /// one-shot on a waitset beside the IRQ endpoint; its fire is the lost-IRQ recovery.
    pub const WATCHDOG: SlotPair = SlotPair::new(0x23, 0x22);
    /// The device tree, read-only (RFC-0098 C3/C7): gpud, the display-mode authority, reads the
    /// lane's request (`/chosen/nexus,display-mode`) here.
    pub const DEVICE_TREE: u32 = 0x24;
    /// The display controller's register window (TASK-0251 P2): the display plane init grants
    /// from the tree (`device.mmio.display`) — empty on a tree without one, where the GPU's
    /// window (`DEVICE_MMIO_SLOT`) is the plane.
    pub const DISPLAY_CONTROLLER: u32 = 0x25;
    /// The display encoder's register window: the pages its block lies in (the block's
    /// in-page offset is its tree node's `reg`).
    pub const DISPLAY_ENCODER: u32 = 0x26;
    /// The CAP_MOVE reply inbox of gpud's one outbound call: socd's bring-up of the display
    /// nodes (TASK-0251 P2, RFC-0106).
    pub const REPLY: SlotPair = SlotPair::new(0x28, 0x27);
    /// The route to socd.
    pub const SOCD: SlotPair = SlotPair::new(0x29, REPLY.recv);
}

/// hidrawd (TASK-0324 P4d; its sources, TASK-0253B). Its loop waits on two endpoints and
/// nothing else — no timer: every grant is in place before it runs, so there is nothing to
/// re-probe, and input is never paced.
pub mod hidrawd {
    use super::SlotPair;

    /// Normalized HID events to inputd (inputd answers on its own endpoint).
    pub const INPUTD: SlotPair = SlotPair::new(3, 4);
    /// RECV half of the virtio-input interrupt endpoint: every granted virtio-input line is
    /// bound to it (`irq_bind`) — a waitset member.
    pub const IRQ_NOTIFY: u32 = 0xF3;
    /// The SUBSCRIBE to xhcid's HID boot class (RFC-0099 §5) on xhcid's server endpoint; the
    /// RECV half is xhcid's shared response endpoint (the answer to a frame it could not read).
    pub const XHCID: SlotPair = SlotPair::new(0xF4, 0xF5);
    /// The USB HID push channel: RECV `0xF6` — a waitset member xhcid pushes the class's
    /// attaches, reports and detaches to — and SEND `0xF7`, moved with the SUBSCRIBE (xhcid
    /// is its only holder from then on).
    pub const USB_HID: SlotPair = SlotPair::new(0xF7, 0xF6);
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
    /// Timer-notify endpoint (TASK-0054C P2-b): the gateway's accept/relay service cadence —
    /// a periodic timer on a waitset beside the server endpoint.
    pub const TIMER: SlotPair = SlotPair::new(10, 9);
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
    /// Timer-notify endpoint (TASK-0324 P7-d): RECV half, a waitset member.
    pub const TIMER_RECV: u32 = 0x23;
    /// Timer-notify endpoint: SEND half the kernel timer cap is bound to.
    pub const TIMER_SEND: u32 = 0x24;
    /// The pair as one value (`nexus_ipc::timer::NotifyTimer::bind`).
    pub const TIMER: SlotPair = SlotPair::new(TIMER_SEND, TIMER_RECV);
    /// The CAP_MOVE reply inbox (RFC-0098 C7): windowd answers inputd's one display-space call
    /// here — never on windowd's shared response endpoint, which several services read.
    pub const REPLY: SlotPair = SlotPair::new(0x26, 0x25);
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
    /// Timer-notify endpoint (TASK-0054C P2-b): the network stack's poll cadence (timers,
    /// retransmits, DHCP) — a periodic timer on a waitset beside the facade endpoint.
    pub const TIMER: SlotPair = SlotPair::new(9, 8);
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

/// `socd` (RFC-0106): the SoC glue owner. Its seven window slots are the fleet
/// constant `SYSCON_MMIO_SLOTS`.
pub mod socd {
    use super::SlotPair;

    /// socd's own server endpoint (consumers send BRING_UP / CLOCK_RATE here).
    pub const SERVER: SlotPair = crate::SERVER_SLOTS;
    /// The shared CAP_MOVE reply inbox.
    pub const REPLY: SlotPair = SlotPair::new(6, 5);
    /// Delegated policy checks (`soc.glue` of the requester).
    pub const POLICYD: SlotPair = SlotPair::new(7, REPLY.recv);
    /// Log sink (optional target).
    pub const LOGD: SlotPair = SlotPair::new(8, REPLY.recv);
    /// The read-only device tree (the alias the kernel gave init).
    pub const DEVICE_TREE: u32 = 0x67;
    /// The one-shot a bring-up's settle is spent on (TASK-0328 U3: a supply's start-up delay,
    /// `vbus-delay-ms`) — a kernel timer waited for on its frame, never a spin.
    pub const TIMER: SlotPair = SlotPair::new(0x69, 0x68);
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
pub mod selftest_client;

/// clipboardd (TASK-0067, RFC-0094): the clipboard authority. A pure server — it asks
/// nobody; identity is the kernel sender id, focus truth arrives as windowd's push.
pub mod clipboardd {
    use super::SlotPair;

    /// clipboardd's own server endpoint.
    pub const SERVER: SlotPair = crate::SERVER_SLOTS;
}

/// screencapd (TASK-0068, RFC-0095): the screen-capture facade. It calls windowd (the capture
/// verb: freeze, thaw, probe, and the lent frame VMO) and vfsd (the PNG file); the shell's
/// `svc.screencap` children and the harness call it.
pub mod screencapd {
    use super::SlotPair;

    /// screencapd's own server endpoint.
    pub const SERVER: SlotPair = crate::SERVER_SLOTS;
    /// The shared CAP_MOVE reply inbox (windowd and vfsd answer on it).
    pub const REPLY: SlotPair = SlotPair::new(6, 5);
    /// The capture verb `OP_SURFACE_CAPTURE` on windowd's surface endpoint.
    pub const WINDOWD: SlotPair = SlotPair::new(7, REPLY.recv);
    /// The screenshot file: create, arm the PNG's VMO, write it.
    pub const VFSD: SlotPair = SlotPair::new(8, REPLY.recv);
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
    /// Timer-notify endpoint (TASK-0324 P7-d): RECV half, a waitset member (the persist
    /// floor + backoff are paced by a kernel one-shot timer, not by a recv timeout).
    pub const TIMER_RECV: u32 = 0x20;
    /// Timer-notify endpoint: SEND half the kernel timer cap is bound to.
    pub const TIMER_SEND: u32 = 0x21;
    /// The pair as one value (`nexus_ipc::timer::NotifyTimer::bind`).
    pub const TIMER: SlotPair = SlotPair::new(TIMER_SEND, TIMER_RECV);
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
    /// The pre-minted CAP_MOVE reply inbox (TASK-0033 P2). vfsd used to read
    /// packagefsd's own RESPONSE endpoint, which dsoftbusd and the harness read
    /// too — three readers on one queue, where any of them could take any
    /// answer. Slot 8 is the request endpoint init mints for execd's children.
    pub const REPLY: SlotPair = SlotPair::new(7, 9);
    /// `pkg:/` resolution and reads: request SEND + the reply inbox's RECV.
    pub const PACKAGEFSD: SlotPair = SlotPair::new(5, REPLY.recv);
}

/// blkd (TASK-0324 P4f-1b; its table is `slots/blkd.rs`).
pub mod blkd;

/// windowd (TASK-0324 P4a; its table is `slots/windowd.rs`).
pub mod windowd;

/// xhcid (TASK-0328 U1; its table is `slots/xhcid.rs`).
pub mod xhcid;
