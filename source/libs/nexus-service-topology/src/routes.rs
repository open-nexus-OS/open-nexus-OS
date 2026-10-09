// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The route graph — which `(from → to)` service links the system provisions and how
//! replies travel on each. Split out of `specs.rs` under the module-size ratchet
//! (TASK-0324 P4f-1b, when the third delivery kind `PrivateInbox` arrived).
//! OWNERS: @runtime
//! STATUS: Production
//! API_STABILITY: Stable within the workspace
//! TEST_COVERAGE: crate tests in lib.rs + `nexus-init` route/policy cross-checks

use crate::{ServiceId, SlotPair};

/// Declarative capability-route SSOT: the `(from → to)` service links the system
/// is expected to provision. Adding a service that needs a route without listing
/// it here (or vice versa) is caught by the host tests below.
pub const REQUIRED_ROUTES: &[(ServiceId, ServiceId)] = &[
    // App lifecycle / registry chain (RFC-0065).
    (ServiceId::Abilitymgr, ServiceId::Bundlemgrd), // resolve installed apps
    (ServiceId::Abilitymgr, ServiceId::Execd),      // spawn app processes
    (ServiceId::Abilitymgr, ServiceId::Sessiond),   // launch gate: session must be active
    // App-child service routing (TASK-0080C): execd resolves these BY NAME on
    // behalf of the app-hosts it spawns — one SEND clone per declared manifest
    // cap into the child's fixed SDK slot (`nexus-sdk-routes`).
    (ServiceId::Execd, ServiceId::Abilitymgr), // svc.ability.* (launcher e2e)
    (ServiceId::Execd, ServiceId::Bundlemgrd), // svc.bundle.* (app enumeration)
    (ServiceId::Execd, ServiceId::Sessiond),   // svc.session.* (DSL greeter login)
    (ServiceId::Execd, ServiceId::Settingsd),  // svc.settings.* (DSL settings app)
    (ServiceId::Execd, ServiceId::Vfsd),       // svc.files.* (filemanager, RFC-0073/TASK-0291)
    (ServiceId::Execd, ServiceId::Statefsd),   // minidump child grant + own dump writes (TASK-0049)
    (ServiceId::Execd, ServiceId::Policyd),    // crash attach-level gate (TASK-0051B, RFC-0087 §5)
    (ServiceId::Execd, ServiceId::Timed),      // svc.time.* / clock tick (RFC-0076)
    (ServiceId::Execd, ServiceId::ImedOsk),    // svc.ime.osk (RFC-0075 Phase 2)
    (ServiceId::Execd, ServiceId::Logd),       // crash-report appends (TASK-0049)
    (ServiceId::Execd, ServiceId::Clipboardd), // svc.clipboard.* (shell search, keyboard; TASK-0067)
    (ServiceId::Execd, ServiceId::Screencapd), // svc.screencap.* (the shell's screenshot UI; TASK-0068)
    // TASK-0324 P4e-2: execd is GRANTED a windowd client route it never calls itself — it
    // clones both halves into every app child (ADR-0042). Provisioned like any other route,
    // so it is declared like one; the delegation is noted on `slots::execd::WINDOWD`.
    (ServiceId::Execd, ServiceId::Windowd),
    (ServiceId::Windowd, ServiceId::Bundlemgrd), // dynamic Apps menu (OP_LIST_APPS)
    (ServiceId::Windowd, ServiceId::Sessiond),   // greeter/login relay (TASK-0065B)
    (ServiceId::Windowd, ServiceId::Settingsd),  // theme GET/SET persistence (TASK-0072 P10)
    (ServiceId::Windowd, ServiceId::Gpud),       // present/attach/cursor handoff (ADR-0032)
    (ServiceId::Windowd, ServiceId::Abilitymgr), // OP_LAUNCH from the shell (TASK-0080D)
    (ServiceId::Windowd, ServiceId::Imed),       // focus relay OP_SET_FOCUS (RFC-0075)
    (ServiceId::Windowd, ServiceId::Clipboardd), // focus truth OP_FOCUS (RFC-0094)
    (ServiceId::Screencapd, ServiceId::Windowd), // capture verb OP_SURFACE_CAPTURE (RFC-0095)
    (ServiceId::Screencapd, ServiceId::Vfsd),    // the PNG: create, arm the VMO, write
    (ServiceId::Inputd, ServiceId::Windowd),     // visible-state push (pointer/keyboard)
    (ServiceId::Inputd, ServiceId::Imed),        // key-forward leg (RFC-0075)
    (ServiceId::Hidrawd, ServiceId::Inputd),     // normalized HID events (RFC-0053)
    // TASK-0253B (RFC-0099 §5): hidrawd subscribes to xhcid's HID boot class; xhcid admits a
    // subscriber by asking policyd for `usb.hid` of the kernel-attributed sender.
    (ServiceId::Hidrawd, ServiceId::Xhcid),
    (ServiceId::Xhcid, ServiceId::Policyd),
    // TASK-0328 U3: the USB host has socd bring the board's host node and hub up (RFC-0106).
    (ServiceId::Xhcid, ServiceId::Socd),
    // RFC-0069 batches 1+2 (regular services migrated onto the declarative arm).
    (ServiceId::Rngd, ServiceId::Logd), // log sink (optional target)
    (ServiceId::Rngd, ServiceId::Policyd), // delegated policy checks
    (ServiceId::Socd, ServiceId::Policyd), // `soc.glue` of the requester (RFC-0106)
    (ServiceId::Socd, ServiceId::Logd), // log sink (optional target)
    // TASK-0246 P4c: the block owner has socd bring its disk's node up (and, on the K1, name
    // the `io` clock's rate) before it touches the controller.
    (ServiceId::Blkd, ServiceId::Socd),
    // TASK-0251 P2: the display owner has socd bring the board's display nodes up (power
    // domain, reset, the demanded clock rate) before it touches the controller.
    (ServiceId::Gpud, ServiceId::Socd),
    (ServiceId::Vfsd, ServiceId::Packagefsd), // pkg:/ metadata + reads (reply inbox)
    (ServiceId::Packagefsd, ServiceId::Bundlemgrd), // slot/manifest queries via CAP_MOVE
    (ServiceId::Samgrd, ServiceId::Logd),     // structured logs via CAP_MOVE
    (ServiceId::Statefsd, ServiceId::Policyd), // policy checks via CAP_MOVE
    (ServiceId::Statefsd, ServiceId::Logd),   // audit trail (TASK-0324 P4f-6)
    (ServiceId::Settingsd, ServiceId::Statefsd), // persist prefs (TASK-0072 Phase 8)
    (ServiceId::Logd, ServiceId::Statefsd),   // evidence spill (TASK-0049C, RFC-0087 §5)
    // TASK-0324 P4f-1b: bootctld's legs are DECLARED routes on fixed slots. bootctld never
    // resolves them (init's boot-attempt handshake runs before the responder serves) — a
    // declared route is a provisioned edge, not a promise to route through the responder.
    (ServiceId::Bootctld, ServiceId::Statefsd), // boot record persistence
    (ServiceId::Bootctld, ServiceId::Policyd),  // boot.target / boot.reset gates
    // TASK-0324 P4f-2: the key store and the policy authority, off their bespoke arms.
    (ServiceId::Keystored, ServiceId::Statefsd), // sealed key store
    (ServiceId::Keystored, ServiceId::Logd),     // structured logs
    (ServiceId::Keystored, ServiceId::Policyd),  // delegated capability checks
    (ServiceId::Keystored, ServiceId::Rngd),     // key-generation entropy
    (ServiceId::Policyd, ServiceId::Logd),       // audit records
    // imed (RFC-0075 / TASK-0204): pushes to windowd, persists the keymap and its ranking blob.
    (ServiceId::Imed, ServiceId::Windowd),
    (ServiceId::Imed, ServiceId::Settingsd),
    (ServiceId::Imed, ServiceId::Statefsd),
    (ServiceId::Updated, ServiceId::Bundlemgrd), // active-slot publication (TASK-0324 P4f-3)
    (ServiceId::Updated, ServiceId::Keystored),  // signature verification
    (ServiceId::Updated, ServiceId::Statefsd),   // persistence
    (ServiceId::Updated, ServiceId::Logd),       // structured logs
    (ServiceId::Bundlemgrd, ServiceId::Logd),    // structured logs (a late grant)
    (ServiceId::Bundlemgrd, ServiceId::Metricsd), // counters (declared, TASK-0324 P7-b)
    (ServiceId::Updated, ServiceId::Bootctld),   // slot mutations delegate (PR-2)
    (ServiceId::Updated, ServiceId::Vfsd),       // staging-source splice reads (TASK-0179)
    (ServiceId::Updated, ServiceId::Policyd),    // updates.manage gate on mutating ops (TASK-0140)
    (ServiceId::Execd, ServiceId::Updated), // svc.updates.* (DSL settings Updates page, TASK-0140)
    (ServiceId::SelftestClient, ServiceId::Bootctld), // reset-lane proof (PR-3)
    // TASK-0324 P4f-5: the proof harness's legs, provisioned for years, declared at last.
    (ServiceId::SelftestClient, ServiceId::Vfsd),
    (ServiceId::SelftestClient, ServiceId::Packagefsd),
    (ServiceId::SelftestClient, ServiceId::Policyd),
    (ServiceId::SelftestClient, ServiceId::Bundlemgrd),
    (ServiceId::SelftestClient, ServiceId::Updated),
    (ServiceId::SelftestClient, ServiceId::Samgrd),
    (ServiceId::SelftestClient, ServiceId::Execd),
    (ServiceId::SelftestClient, ServiceId::Keystored),
    (ServiceId::SelftestClient, ServiceId::Statefsd),
    (ServiceId::SelftestClient, ServiceId::Logd),
    (ServiceId::SelftestClient, ServiceId::Inputd),
    (ServiceId::SelftestClient, ServiceId::Netstackd),
    (ServiceId::SelftestClient, ServiceId::Dsoftbusd),
    (ServiceId::SelftestClient, ServiceId::Rngd),
    (ServiceId::SelftestClient, ServiceId::Socd), // NotNeeded on a tree without glue
    (ServiceId::SelftestClient, ServiceId::Timed),
    (ServiceId::SelftestClient, ServiceId::Metricsd),
    (ServiceId::SelftestClient, ServiceId::Pinched),
    (ServiceId::SelftestClient, ServiceId::Settingsd),
    (ServiceId::SelftestClient, ServiceId::Imed),
    (ServiceId::SelftestClient, ServiceId::ImedOsk),
    (ServiceId::SelftestClient, ServiceId::Blkd),
    (ServiceId::SelftestClient, ServiceId::Clipboardd), // the gate's deny side + the prefill
    (ServiceId::SelftestClient, ServiceId::Screencapd), // the non-black probe through the readback
    // RFC-0092 (TASK-0052 P3): the ingress gateway asks policyd for the
    // declared subject's `net.expose` and drives netstackd (listen/accept/
    // connect/relay); the selftest registers its exposure intents.
    // TASK-0324 P4f-4: the network edge and telemetry, off their bespoke arms.
    (ServiceId::Netstackd, ServiceId::Policyd), // RFC-0091 seam
    (ServiceId::Dsoftbusd, ServiceId::Netstackd), // facade client
    (ServiceId::Dsoftbusd, ServiceId::Samgrd),  // registry lookups
    (ServiceId::Dsoftbusd, ServiceId::Bundlemgrd), // bundle queries
    (ServiceId::Dsoftbusd, ServiceId::Packagefsd), // remote packagefs RO path (TASK-0016)
    (ServiceId::Dsoftbusd, ServiceId::Statefsd), // remote statefs proxy (TASK-0017)
    (ServiceId::Dsoftbusd, ServiceId::Logd),    // structured logs
    (ServiceId::Metricsd, ServiceId::Statefsd), // retention writer
    (ServiceId::Metricsd, ServiceId::Logd),     // snapshot/span export
    (ServiceId::Ingressd, ServiceId::Policyd),
    (ServiceId::Ingressd, ServiceId::Netstackd),
    (ServiceId::SelftestClient, ServiceId::Ingressd),
];

/// How a service receives the target's replies on a declared route (RFC-0069).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteKind {
    /// Send on the target's request endpoint; replies arrive on the caller's
    /// CAP_MOVE reply inbox (requires `reply_inbox`).
    ReplyInbox,
    /// Send on the target's request endpoint; replies arrive on the target's
    /// pre-minted RESPONSE endpoint, shared directly (no reply inbox).
    SharedResponse,
    /// Send on the target's request endpoint; replies arrive on an inbox PRIVATE to this
    /// route — `slots.recv` is its RECV half, `inbox_send` the SEND half the caller moves
    /// with each request (TASK-0324 P4f-1b). Used where two outbound legs of one service must
    /// never share a reply queue (imed's settingsd and statefsd legs: a slow statefs PUT must
    /// not swallow a settings reply). It was provisioned by hand before it had a name.
    PrivateInbox {
        /// Slot of the private inbox's SEND half.
        inbox_send: u32,
    },
}

/// One declared outbound route (must appear in [`REQUIRED_ROUTES`]).
#[derive(Clone, Copy, Debug)]
pub struct Route {
    /// The callee.
    pub to: ServiceId,
    /// How replies come back.
    pub kind: RouteKind,
    /// Capability slots the requester receives for this route; every route of a spec declares
    /// them (`test_reject_partial_slot_declaration`, TASK-0324 P4).
    pub slots: SlotPair,
}
