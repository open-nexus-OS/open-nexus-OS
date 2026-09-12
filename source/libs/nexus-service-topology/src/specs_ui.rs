// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Service declarations for the display and input chain — one named `ServiceSpec` per service;
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

/// The declaration of `hidrawd`.
pub(crate) const HIDRAWD: ServiceSpec = ServiceSpec {
    id: ServiceId::Hidrawd,
    exposes_server: false,
    reply_inbox: false,
    // TASK-0324 P4d: a pure producer — it pushes normalized HID events to inputd and
    // exposes no endpoint of its own. Its three virtio-input MMIO windows come from the
    // fleet-wide `INPUT_MMIO_SLOT_BASE` block, so they are not per-service grants.
    routes_to: &[Route {
        to: ServiceId::Inputd,
        kind: RouteKind::SharedResponse,
        slots: slots::hidrawd::INPUTD,
    }],
    announce: false,
    server_slots: SlotPair::UNDECLARED,
    reply_slots: SlotPair::UNDECLARED,
    extra_slots: &[],
};

/// The declaration of `gpud`.
pub(crate) const GPUD: ServiceSpec = ServiceSpec {
    id: ServiceId::Gpud,
    exposes_server: true,
    reply_inbox: false,
    // TASK-0324 P4c: a pure server — windowd calls it, it calls nobody. Its MMIO window
    // uses the fleet-wide `DEVICE_MMIO_SLOT`; its IRQ notification deliberately reuses
    // the control-reply endpoint (never the server endpoint, which would swallow
    // windowd's present commands), so neither is a per-service grant to declare here.
    routes_to: &[],
    announce: true,
    server_slots: slots::gpud::SERVER,
    reply_slots: SlotPair::UNDECLARED,
    extra_slots: &[],
};

/// The declaration of `inputd`.
pub(crate) const INPUTD: ServiceSpec = ServiceSpec {
    id: ServiceId::Inputd,
    exposes_server: true,
    reply_inbox: false,
    // TASK-0324 P4b: inputd had NO declaration at all — init wired it entirely from a
    // bespoke arm whose comments called the windowd leg's slot numbers "a boot
    // contract". They are that contract now, in one readable place.
    routes_to: &[
        Route {
            to: ServiceId::Windowd,
            kind: RouteKind::SharedResponse,
            slots: slots::inputd::WINDOWD,
        },
        Route { to: ServiceId::Imed, kind: RouteKind::SharedResponse, slots: slots::inputd::IMED },
    ],
    announce: true,
    server_slots: slots::inputd::SERVER,
    reply_slots: SlotPair::UNDECLARED,
    extra_slots: &[
        NamedSlotBinding { name: NamedSlot::Settings, slot: slots::inputd::SETTINGS_SEND },
        NamedSlotBinding { name: NamedSlot::SettingsWatchRecv, slot: slots::inputd::WATCH_RECV },
        NamedSlotBinding { name: NamedSlot::SettingsWatchSend, slot: slots::inputd::WATCH_SEND },
    ],
};

/// The declaration of `windowd`.
pub(crate) const WINDOWD: ServiceSpec = ServiceSpec {
    id: ServiceId::Windowd,
    exposes_server: true,
    reply_inbox: true,
    // TASK-0324 P4a: windowd is the FIRST consumer on the declared arm. The slots below
    // are the ones init used to hand out by TRANSFER ORDER — a contract nobody could
    // read, and one that broke the display handoff when a route was provisioned a step
    // too early ("shifted gpud to 8/9 → present handoff kernel-permission-denied").
    // They are declared here and pinned by init (`cap_transfer_to_slot`), so order is
    // irrelevant and a collision is a test failure instead of a black screen.
    routes_to: &[
        Route { to: ServiceId::Gpud, kind: RouteKind::SharedResponse, slots: slots::windowd::GPUD },
        Route {
            to: ServiceId::Bundlemgrd,
            kind: RouteKind::ReplyInbox,
            slots: slots::windowd::BUNDLEMGRD,
        },
        Route {
            to: ServiceId::Sessiond,
            kind: RouteKind::ReplyInbox,
            slots: slots::windowd::SESSIOND,
        },
        Route {
            to: ServiceId::Settingsd,
            kind: RouteKind::ReplyInbox,
            slots: slots::windowd::SETTINGSD,
        },
        Route {
            to: ServiceId::Abilitymgr,
            kind: RouteKind::SharedResponse,
            slots: slots::windowd::ABILITYMGR,
        },
        Route { to: ServiceId::Imed, kind: RouteKind::ReplyInbox, slots: slots::windowd::IMED },
    ],
    announce: true,
    server_slots: slots::windowd::SERVER,
    reply_slots: slots::windowd::REPLY,
    extra_slots: &[
        NamedSlotBinding { name: NamedSlot::SettingsWatchRecv, slot: slots::windowd::WATCH_RECV },
        NamedSlotBinding { name: NamedSlot::SettingsWatchSend, slot: slots::windowd::WATCH_SEND },
    ],
};

// RFC-0075: imed's server pair is pre-minted; its windowd client leg is
// provisioned in the generic arm (fire-and-forget pushes, no reply inbox).
/// The declaration of `imed`.
pub(crate) const IMED: ServiceSpec = ServiceSpec {
    id: ServiceId::Imed,
    exposes_server: true,
    reply_inbox: false,
    routes_to: &[
        Route {
            to: ServiceId::Windowd,
            kind: RouteKind::SharedResponse,
            slots: slots::imed::WINDOWD,
        },
        Route {
            to: ServiceId::Settingsd,
            kind: RouteKind::PrivateInbox { inbox_send: slots::imed::SETTINGSD_INBOX_SEND },
            slots: slots::imed::SETTINGSD,
        },
        Route {
            to: ServiceId::Statefsd,
            kind: RouteKind::PrivateInbox { inbox_send: slots::imed::STATEFSD_INBOX_SEND },
            slots: slots::imed::STATEFSD,
        },
    ],
    announce: false,
    server_slots: slots::imed::SERVER,
    reply_slots: SlotPair::UNDECLARED,
    extra_slots: &[NamedSlotBinding {
        name: NamedSlot::OskServerRecv,
        slot: slots::imed::OSK_RECV,
    }],
};

/// The declaration of `touchd` (TASK-0324 P4f-6): it holds nothing beyond the control channel.
/// Declared anyway, so that no service is invisible to the slot tests.
pub(crate) const TOUCHD: ServiceSpec = ServiceSpec {
    id: ServiceId::Touchd,
    exposes_server: false,
    reply_inbox: false,
    routes_to: &[],
    announce: false,
    server_slots: SlotPair::UNDECLARED,
    reply_slots: SlotPair::UNDECLARED,
    extra_slots: &[],
};
