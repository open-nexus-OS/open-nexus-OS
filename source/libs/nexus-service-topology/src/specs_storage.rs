// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Service declarations for storage and boot — one named `ServiceSpec` per service;
//! `specs::SERVICE_SPECS` lists them. Split per plane under the module-size ratchet
//! (TASK-0324 P4f-2) so the table can keep growing without a monolith.
//! OWNERS: @runtime
//! STATUS: Production
//! API_STABILITY: Stable within the workspace
//! TEST_COVERAGE: crate tests in lib.rs + `nexus-init` route/policy cross-checks

use crate::routes::Route;
use crate::routes::RouteKind;
use crate::specs::ServiceSpec;
use crate::Stage;
use crate::{slots, NamedSlot, NamedSlotBinding, ServiceId, SlotPair};

/// The declaration of `keystored`.
pub(crate) const KEYSTORED: ServiceSpec = ServiceSpec {
    id: ServiceId::Keystored,
    stage: Stage::Platform,
    exposes_server: true,
    reply_inbox: true,
    // TASK-0324 P4f-2: its bespoke arm (~190 lines, mostly transfer tracing) is deleted; the
    // generic arm provisions it from this declaration.
    routes_to: &[
        Route {
            to: ServiceId::Statefsd,
            kind: RouteKind::ReplyInbox,
            slots: slots::keystored::STATEFSD,
        },
        Route { to: ServiceId::Logd, kind: RouteKind::ReplyInbox, slots: slots::keystored::LOGD },
        Route {
            to: ServiceId::Policyd,
            kind: RouteKind::ReplyInbox,
            slots: slots::keystored::POLICYD,
        },
        Route { to: ServiceId::Rngd, kind: RouteKind::ReplyInbox, slots: slots::keystored::RNGD },
    ],
    announce: true,
    server_slots: slots::keystored::SERVER,
    reply_slots: slots::keystored::REPLY,
    extra_slots: &[],
};

/// The declaration of `vfsd`.
pub(crate) const VFSD: ServiceSpec = ServiceSpec {
    id: ServiceId::Vfsd,
    stage: Stage::Platform,
    exposes_server: true,
    reply_inbox: false,
    routes_to: &[Route {
        to: ServiceId::Packagefsd,
        kind: RouteKind::SharedResponse,
        slots: slots::vfsd::PACKAGEFSD,
    }],
    announce: false,
    server_slots: slots::vfsd::SERVER,
    reply_slots: SlotPair::UNDECLARED,
    extra_slots: &[],
};

/// The declaration of `packagefsd`.
pub(crate) const PACKAGEFSD: ServiceSpec = ServiceSpec {
    id: ServiceId::Packagefsd,
    stage: Stage::Platform,
    exposes_server: true,
    reply_inbox: true,
    routes_to: &[Route {
        to: ServiceId::Bundlemgrd,
        kind: RouteKind::ReplyInbox,
        slots: slots::packagefsd::BUNDLEMGRD,
    }],
    announce: false,
    server_slots: slots::packagefsd::SERVER,
    reply_slots: slots::packagefsd::REPLY,
    extra_slots: &[],
};

/// The declaration of `statefsd`.
pub(crate) const STATEFSD: ServiceSpec = ServiceSpec {
    id: ServiceId::Statefsd,
    stage: Stage::Platform,
    exposes_server: true,
    reply_inbox: true,
    routes_to: &[
        Route {
            to: ServiceId::Policyd,
            kind: RouteKind::ReplyInbox,
            slots: slots::statefsd::POLICYD,
        },
        Route { to: ServiceId::Logd, kind: RouteKind::ReplyInbox, slots: slots::statefsd::LOGD },
    ],
    announce: true,
    server_slots: slots::statefsd::SERVER,
    reply_slots: slots::statefsd::REPLY,
    extra_slots: &[],
};

// TASK-0315: the block-plane owner. `reply_inbox` despite empty routes:
// the inbox is the driver's IRQ notify endpoint (this service makes no
// outbound calls, so the channel is exclusively the interrupt wake).
/// The declaration of `virtioblkd`.
pub(crate) const VIRTIOBLKD: ServiceSpec = ServiceSpec {
    id: ServiceId::Virtioblkd,
    stage: Stage::Platform,
    exposes_server: true,
    reply_inbox: false,
    routes_to: &[],
    announce: true,
    server_slots: slots::virtioblkd::SERVER,
    reply_slots: SlotPair::UNDECLARED,
    extra_slots: &[
        NamedSlotBinding { name: NamedSlot::IrqNotify, slot: slots::virtioblkd::IRQ_NOTIFY },
        NamedSlotBinding {
            name: NamedSlot::DeviceWatchdogRecv,
            slot: slots::virtioblkd::WATCHDOG.recv,
        },
        NamedSlotBinding {
            name: NamedSlot::DeviceWatchdogSend,
            slot: slots::virtioblkd::WATCHDOG.send,
        },
    ],
};

// Batch 4 (amended by TASK-0049C): logd persists evidence-class records
// to statefsd (spill txns via its CAP_MOVE reply inbox — never the
// shared response queue).
/// The declaration of `logd`.
pub(crate) const LOGD: ServiceSpec = ServiceSpec {
    id: ServiceId::Logd,
    stage: Stage::Platform,
    exposes_server: true,
    reply_inbox: true,
    routes_to: &[Route {
        to: ServiceId::Statefsd,
        kind: RouteKind::ReplyInbox,
        slots: slots::logd::STATEFSD,
    }],
    announce: true,
    server_slots: slots::logd::SERVER,
    reply_slots: slots::logd::REPLY,
    extra_slots: &[],
};

// bootctld: single boot-state authority (TASK-0050, ADR-0055). Exposes a
// server (updated/init/selftest are clients). BESPOKE-wired with FIXED
// slots (inbox 5/6, statefsd send 7): its statefs attach must not
// resolve routes — init calls the boot-attempt handshake before the
// responder serves, so a responder-dependent attach would deadlock.
/// The declaration of `bootctld`.
pub(crate) const BOOTCTLD: ServiceSpec = ServiceSpec {
    id: ServiceId::Bootctld,
    stage: Stage::Platform,
    exposes_server: true,
    reply_inbox: true,
    routes_to: &[
        Route {
            to: ServiceId::Statefsd,
            kind: RouteKind::ReplyInbox,
            slots: slots::bootctld::STATEFSD,
        },
        Route {
            to: ServiceId::Policyd,
            kind: RouteKind::ReplyInbox,
            slots: slots::bootctld::POLICYD,
        },
    ],
    announce: true,
    server_slots: slots::bootctld::SERVER,
    reply_slots: slots::bootctld::REPLY,
    extra_slots: &[],
};

/// The declaration of `updated`.
// TASK-0324 P4f-3: off its bespoke arm; every leg is declared and pinned by the generic arm.
pub(crate) const UPDATED: ServiceSpec = ServiceSpec {
    id: ServiceId::Updated,
    stage: Stage::Platform,
    exposes_server: true,
    reply_inbox: true,
    routes_to: &[
        Route {
            to: ServiceId::Bundlemgrd,
            kind: RouteKind::ReplyInbox,
            slots: slots::updated::BUNDLEMGRD,
        },
        Route {
            to: ServiceId::Keystored,
            kind: RouteKind::SharedResponse,
            slots: slots::updated::KEYSTORED,
        },
        Route {
            to: ServiceId::Statefsd,
            kind: RouteKind::ReplyInbox,
            slots: slots::updated::STATEFSD,
        },
        Route { to: ServiceId::Vfsd, kind: RouteKind::ReplyInbox, slots: slots::updated::VFSD },
        Route {
            to: ServiceId::Bootctld,
            kind: RouteKind::ReplyInbox,
            slots: slots::updated::BOOTCTLD,
        },
        Route {
            to: ServiceId::Policyd,
            kind: RouteKind::ReplyInbox,
            slots: slots::updated::POLICYD,
        },
        Route { to: ServiceId::Logd, kind: RouteKind::ReplyInbox, slots: slots::updated::LOGD },
    ],
    announce: true,
    server_slots: slots::updated::SERVER,
    reply_slots: slots::updated::REPLY,
    extra_slots: &[],
};
