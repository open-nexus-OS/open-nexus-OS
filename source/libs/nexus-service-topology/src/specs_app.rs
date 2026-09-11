// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Service declarations for the app platform — one named `ServiceSpec` per service;
//! `specs::SERVICE_SPECS` lists them. Split per plane under the module-size ratchet
//! (TASK-0324 P4f-2) so the table can keep growing without a monolith.
//! OWNERS: @runtime
//! STATUS: Production
//! API_STABILITY: Stable within the workspace
//! TEST_COVERAGE: crate tests in lib.rs + `nexus-init` route/policy cross-checks

use crate::routes::Route;
use crate::routes::RouteKind;
use crate::specs::ServiceSpec;
use crate::{slots, NamedSlot, NamedSlotBinding, ServiceId, SlotPair};

/// The declaration of `abilitymgr`.
pub(crate) const ABILITYMGR: ServiceSpec = ServiceSpec {
    id: ServiceId::Abilitymgr,
    exposes_server: true,
    reply_inbox: true,
    routes_to: &[
        Route {
            to: ServiceId::Bundlemgrd,
            kind: RouteKind::ReplyInbox,
            slots: slots::abilitymgr::BUNDLEMGRD,
        },
        Route {
            to: ServiceId::Execd,
            kind: RouteKind::SharedResponse,
            slots: slots::abilitymgr::EXECD,
        },
        Route {
            to: ServiceId::Sessiond,
            kind: RouteKind::ReplyInbox,
            slots: slots::abilitymgr::SESSIOND,
        },
    ],
    announce: true,
    server_slots: slots::abilitymgr::SERVER,
    reply_slots: slots::abilitymgr::REPLY,
    extra_slots: &[],
};

/// The declaration of `execd`.
pub(crate) const EXECD: ServiceSpec = ServiceSpec {
    id: ServiceId::Execd,
    exposes_server: true,
    reply_inbox: true,
    // TASK-0324 P4e-2: execd's OWN table (P4e declared the table of the children it
    // spawns). It had no spec at all — every one of the numbers below was a transfer
    // position in init's bespoke arm, mirrored by a `const … SLOT` in execd and held in
    // place by comments ("keep this block FIRST", "ARM END on purpose"). The named
    // routes at 15+ travel back in the route response, so execd resolves them BY NAME
    // and never hardcodes them; they are declared because init must still put them
    // somewhere, and "somewhere" was previously whatever slot the last transfer left.
    routes_to: &[
        Route { to: ServiceId::Logd, kind: RouteKind::ReplyInbox, slots: slots::execd::LOGD },
        Route {
            to: ServiceId::Windowd,
            kind: RouteKind::SharedResponse,
            slots: slots::execd::WINDOWD,
        },
        Route {
            to: ServiceId::Bundlemgrd,
            kind: RouteKind::ReplyInbox,
            slots: slots::execd::BUNDLEMGRD,
        },
        Route {
            to: ServiceId::Abilitymgr,
            kind: RouteKind::ReplyInbox,
            slots: slots::execd::ABILITYMGR,
        },
        Route {
            to: ServiceId::Sessiond,
            kind: RouteKind::ReplyInbox,
            slots: slots::execd::SESSIOND,
        },
        Route {
            to: ServiceId::Settingsd,
            kind: RouteKind::ReplyInbox,
            slots: slots::execd::SETTINGSD,
        },
        Route { to: ServiceId::Timed, kind: RouteKind::ReplyInbox, slots: slots::execd::TIMED },
        Route {
            to: ServiceId::ImedOsk,
            kind: RouteKind::ReplyInbox,
            slots: slots::execd::IMED_OSK,
        },
        Route { to: ServiceId::Vfsd, kind: RouteKind::ReplyInbox, slots: slots::execd::VFSD },
        Route { to: ServiceId::Updated, kind: RouteKind::ReplyInbox, slots: slots::execd::UPDATED },
        Route {
            to: ServiceId::Statefsd,
            kind: RouteKind::SharedResponse,
            slots: slots::execd::STATEFSD,
        },
        Route { to: ServiceId::Policyd, kind: RouteKind::ReplyInbox, slots: slots::execd::POLICYD },
    ],
    announce: true,
    server_slots: slots::execd::SERVER,
    reply_slots: slots::execd::REPLY,
    extra_slots: &[
        NamedSlotBinding { name: NamedSlot::ProbePingSend, slot: slots::execd::PROBE_PING.send },
        NamedSlotBinding { name: NamedSlot::ProbePingRecv, slot: slots::execd::PROBE_PING.recv },
        NamedSlotBinding { name: NamedSlot::ProbeReplySend, slot: slots::execd::PROBE_REPLY.send },
        NamedSlotBinding { name: NamedSlot::ProbeReplyRecv, slot: slots::execd::PROBE_REPLY.recv },
    ],
};

/// The declaration of `timed`.
pub(crate) const TIMED: ServiceSpec = ServiceSpec {
    id: ServiceId::Timed,
    exposes_server: true,
    reply_inbox: false,
    routes_to: &[],
    announce: false,
    server_slots: slots::timed::SERVER,
    reply_slots: SlotPair::UNDECLARED,
    extra_slots: &[],
};

// Batch 3: their deleted arms printed the iw-gated `init: <svc> slots …`
// line — announce=true keeps print + init_caps tally parity.
/// The declaration of `samgrd`.
pub(crate) const SAMGRD: ServiceSpec = ServiceSpec {
    id: ServiceId::Samgrd,
    exposes_server: true,
    reply_inbox: true,
    routes_to: &[Route {
        to: ServiceId::Logd,
        kind: RouteKind::ReplyInbox,
        slots: slots::samgrd::LOGD,
    }],
    announce: true,
    server_slots: slots::samgrd::SERVER,
    reply_slots: slots::samgrd::REPLY,
    extra_slots: &[],
};

// Batch S (RFC-0069 §4): the session manager — a NEW service that is
// nothing but this manifest entry on the init side (the whole point of the
// declarative arm). Owns the `session-start` stage; today it auto-starts
// the default session. The greeter/login docks onto its server endpoint.
/// The declaration of `sessiond`.
pub(crate) const SESSIOND: ServiceSpec = ServiceSpec {
    id: ServiceId::Sessiond,
    exposes_server: true,
    reply_inbox: false,
    routes_to: &[],
    announce: true,
    server_slots: slots::sessiond::SERVER,
    reply_slots: SlotPair::UNDECLARED,
    extra_slots: &[],
};

// TASK-0072 Phase 8: the typed settings registry. Exposes a server (windowd
// settings panel is a client, Phase 10) and calls statefsd to persist prefs
// (its reply inbox = the shared `@reply` recipe). New service = this manifest
// entry + its statefsd route + policy grant; the declarative arm wires it.
/// The declaration of `settingsd`.
pub(crate) const SETTINGSD: ServiceSpec = ServiceSpec {
    id: ServiceId::Settingsd,
    exposes_server: true,
    reply_inbox: true,
    routes_to: &[Route {
        to: ServiceId::Statefsd,
        kind: RouteKind::ReplyInbox,
        slots: slots::settingsd::STATEFSD,
    }],
    announce: false,
    server_slots: slots::settingsd::SERVER,
    reply_slots: slots::settingsd::REPLY,
    extra_slots: &[],
};

// pinched: system-internal compute broker (SMP track Phase D). Exposes a
// server for system clients (selftest, SDK batch paths); calls nobody —
// its parallelism is in-process threads on nexus-workpool, not IPC.
// Deliberately NOT in nexus-sdk-routes: apps must never see it.
/// The declaration of `pinched`.
pub(crate) const PINCHED: ServiceSpec = ServiceSpec {
    id: ServiceId::Pinched,
    exposes_server: true,
    reply_inbox: false,
    routes_to: &[],
    announce: false,
    server_slots: slots::pinched::SERVER,
    reply_slots: SlotPair::UNDECLARED,
    extra_slots: &[],
};
