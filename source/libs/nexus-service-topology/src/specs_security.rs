// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Service declarations for the policy authority, entropy and the network edge —
//! one named `ServiceSpec` per service; `specs::SERVICE_SPECS` lists them. Split per plane under the module-size ratchet
//! (TASK-0324 P4f-2) so the table can keep growing without a monolith.
//! OWNERS: @runtime
//! STATUS: Production
//! API_STABILITY: Stable within the workspace
//! TEST_COVERAGE: crate tests in lib.rs + `nexus-init` route/policy cross-checks

use crate::routes::Route;
use crate::routes::RouteKind;
use crate::specs::ServiceSpec;
use crate::Stage;
use crate::{slots, NamedSlot, NamedSlotBinding, ServiceId};

/// The declaration of `policyd`.
pub(crate) const POLICYD: ServiceSpec = ServiceSpec {
    id: ServiceId::Policyd,
    stage: Stage::Platform,
    exposes_server: true,
    reply_inbox: true,
    // TASK-0324 P4f-2: the policy authority. Its server pair and init's two check channels
    // are pinned in the core plane (policyd runs before the wiring phase); the generic arm
    // provisions the audit inbox and the logd leg. It never routes through the responder,
    // which is why it holds no `ipc.core` grant (see the policy coverage test).
    routes_to: &[Route {
        to: ServiceId::Logd,
        kind: RouteKind::ReplyInbox,
        slots: slots::policyd::LOGD,
    }],
    announce: true,
    server_slots: slots::policyd::SERVER,
    reply_slots: slots::policyd::REPLY,
    extra_slots: &[
        NamedSlotBinding {
            name: NamedSlot::PolicyRouteCheckRecv,
            slot: slots::policyd::ROUTE_CHECK.recv,
        },
        NamedSlotBinding {
            name: NamedSlot::PolicyRouteCheckSend,
            slot: slots::policyd::ROUTE_CHECK.send,
        },
        NamedSlotBinding {
            name: NamedSlot::PolicyExecCheckRecv,
            slot: slots::policyd::EXEC_CHECK.recv,
        },
        NamedSlotBinding {
            name: NamedSlot::PolicyExecCheckSend,
            slot: slots::policyd::EXEC_CHECK.send,
        },
    ],
};

// RFC-0069 batches 1+2: regular services wired ENTIRELY from the spec (the
// bespoke arms are deleted). Their server pair is PRE-MINTED (see
// `Endpoints::server_pair`) — the generic arm transfers it instead of
// creating a fresh endpoint; `announce: false` because the deleted arms
// printed nothing.
/// The declaration of `rngd`.
pub(crate) const RNGD: ServiceSpec = ServiceSpec {
    id: ServiceId::Rngd,
    stage: Stage::Platform,
    exposes_server: true,
    reply_inbox: true,
    routes_to: &[
        Route { to: ServiceId::Logd, kind: RouteKind::ReplyInbox, slots: slots::rngd::LOGD },
        Route { to: ServiceId::Policyd, kind: RouteKind::ReplyInbox, slots: slots::rngd::POLICYD },
    ],
    announce: false,
    server_slots: slots::rngd::SERVER,
    reply_slots: slots::rngd::REPLY,
    extra_slots: &[],
};

// RFC-0092 (TASK-0052 P3): both legs are CAP_MOVE reply-inbox routes —
// policyd answers the delegated `net.expose` check, netstackd answers
// every facade RPC on the same inbox (the gateway drains one RPC at a
// time, so the two never interleave).
/// The declaration of `ingressd`.
pub(crate) const INGRESSD: ServiceSpec = ServiceSpec {
    id: ServiceId::Ingressd,
    stage: Stage::Platform,
    exposes_server: true,
    reply_inbox: true,
    routes_to: &[
        Route {
            to: ServiceId::Policyd,
            kind: RouteKind::ReplyInbox,
            slots: slots::ingressd::POLICYD,
        },
        Route {
            to: ServiceId::Netstackd,
            kind: RouteKind::ReplyInbox,
            slots: slots::ingressd::NETSTACKD,
        },
    ],
    announce: true,
    server_slots: slots::ingressd::SERVER,
    reply_slots: slots::ingressd::REPLY,
    extra_slots: &[
        NamedSlotBinding { name: NamedSlot::TimerNotifyRecv, slot: slots::ingressd::TIMER.recv },
        NamedSlotBinding { name: NamedSlot::TimerNotifySend, slot: slots::ingressd::TIMER.send },
    ],
};

/// The declaration of `netstackd`.
// TASK-0324 P4f-4: off its bespoke arm (literal pins 5/6 + 7/8/9, plus a duplicate server pair).
pub(crate) const NETSTACKD: ServiceSpec = ServiceSpec {
    id: ServiceId::Netstackd,
    stage: Stage::Platform,
    exposes_server: true,
    reply_inbox: true,
    routes_to: &[Route {
        to: ServiceId::Policyd,
        kind: RouteKind::ReplyInbox,
        slots: slots::netstackd::POLICYD,
    }],
    announce: true,
    server_slots: slots::netstackd::SERVER,
    reply_slots: slots::netstackd::REPLY,
    extra_slots: &[
        NamedSlotBinding { name: NamedSlot::TimerNotifyRecv, slot: slots::netstackd::TIMER.recv },
        NamedSlotBinding { name: NamedSlot::TimerNotifySend, slot: slots::netstackd::TIMER.send },
    ],
};

/// The declaration of `dsoftbusd`.
// TASK-0324 P4f-4: off its bespoke arm.
pub(crate) const DSOFTBUSD: ServiceSpec = ServiceSpec {
    id: ServiceId::Dsoftbusd,
    stage: Stage::Platform,
    exposes_server: true,
    reply_inbox: true,
    routes_to: &[
        Route {
            to: ServiceId::Netstackd,
            kind: RouteKind::ReplyInbox,
            slots: slots::dsoftbusd::NETSTACKD,
        },
        Route {
            to: ServiceId::Samgrd,
            kind: RouteKind::ReplyInbox,
            slots: slots::dsoftbusd::SAMGRD,
        },
        Route {
            to: ServiceId::Bundlemgrd,
            kind: RouteKind::ReplyInbox,
            slots: slots::dsoftbusd::BUNDLEMGRD,
        },
        Route {
            to: ServiceId::Packagefsd,
            kind: RouteKind::SharedResponse,
            slots: slots::dsoftbusd::PACKAGEFSD,
        },
        Route {
            to: ServiceId::Statefsd,
            kind: RouteKind::SharedResponse,
            slots: slots::dsoftbusd::STATEFSD,
        },
        Route { to: ServiceId::Logd, kind: RouteKind::ReplyInbox, slots: slots::dsoftbusd::LOGD },
    ],
    announce: true,
    server_slots: slots::dsoftbusd::SERVER,
    reply_slots: slots::dsoftbusd::REPLY,
    extra_slots: &[],
};
