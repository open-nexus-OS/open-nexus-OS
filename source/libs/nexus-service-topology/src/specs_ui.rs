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
use crate::Stage;
use crate::{slots, NamedSlot, NamedSlotBinding, ServiceId, SlotPair};

/// The declaration of `hidrawd`.
pub(crate) const HIDRAWD: ServiceSpec = ServiceSpec {
    id: ServiceId::Hidrawd,
    stage: Stage::DisplayReady,
    exposes_server: false,
    reply_inbox: false,
    // TASK-0324 P4d: a pure producer — it pushes normalized HID events to inputd and
    // exposes no endpoint of its own. Its three virtio-input MMIO windows come from the
    // fleet-wide `INPUT_MMIO_SLOT_BASE` block, so they are not per-service grants.
    // TASK-0253B: its sources wake it — the virtio-input lines on one notify endpoint, xhcid's
    // HID boot class on a push channel it subscribes to once (RFC-0099 §5).
    routes_to: &[
        Route {
            to: ServiceId::Inputd,
            kind: RouteKind::SharedResponse,
            slots: slots::hidrawd::INPUTD,
        },
        Route {
            to: ServiceId::Xhcid,
            kind: RouteKind::SharedResponse,
            slots: slots::hidrawd::XHCID,
        },
    ],
    announce: false,
    server_slots: SlotPair::UNDECLARED,
    reply_slots: SlotPair::UNDECLARED,
    extra_slots: &[
        NamedSlotBinding { name: NamedSlot::IrqNotify, slot: slots::hidrawd::IRQ_NOTIFY },
        NamedSlotBinding { name: NamedSlot::UsbHidRecv, slot: slots::hidrawd::USB_HID.recv },
        NamedSlotBinding { name: NamedSlot::UsbHidSend, slot: slots::hidrawd::USB_HID.send },
    ],
};

/// The declaration of `xhcid` (TASK-0328 U1, RFC-0099): the USB host controller's one owner.
/// Its controller window is the fleet-wide `DEVICE_MMIO_SLOT` (`device.mmio.usb`); the
/// reactive loop waits on the controller's line, a one-shot and its server endpoint, all pinned
/// before it runs. TASK-0253B: it serves the HID boot class to one subscriber (RFC-0099 §5),
/// admitted by policyd (`usb.hid` of the kernel-attributed sender) — so it is a member of the
/// DisplayReady barrier and announces as soon as it serves, on every lane (without a
/// controller too). On the board (U3) it has socd bring the host node and the on-board hub up
/// (RFC-0106) before it touches the controller, reading both nodes from its own tree slot.
pub(crate) const XHCID: ServiceSpec = ServiceSpec {
    id: ServiceId::Xhcid,
    stage: Stage::DisplayReady,
    exposes_server: true,
    reply_inbox: true,
    routes_to: &[
        Route { to: ServiceId::Policyd, kind: RouteKind::ReplyInbox, slots: slots::xhcid::POLICYD },
        Route { to: ServiceId::Socd, kind: RouteKind::ReplyInbox, slots: slots::xhcid::SOCD },
    ],
    announce: false,
    server_slots: slots::xhcid::SERVER,
    reply_slots: slots::xhcid::REPLY,
    extra_slots: &[
        NamedSlotBinding { name: NamedSlot::DeviceTree, slot: slots::xhcid::DEVICE_TREE },
        NamedSlotBinding { name: NamedSlot::IrqNotify, slot: slots::xhcid::IRQ_NOTIFY },
        NamedSlotBinding { name: NamedSlot::TimerNotifyRecv, slot: slots::xhcid::TIMER.recv },
        NamedSlotBinding { name: NamedSlot::TimerNotifySend, slot: slots::xhcid::TIMER.send },
    ],
};

/// The declaration of `gpud`.
pub(crate) const GPUD: ServiceSpec = ServiceSpec {
    id: ServiceId::Gpud,
    stage: Stage::DisplayReady,
    exposes_server: true,
    // TASK-0251 P2: its one outbound call is socd's bring-up of the board's display nodes
    // (RFC-0106), answered on its own inbox. Its GPU window uses the fleet-wide
    // `DEVICE_MMIO_SLOT`, the board's display plane `slots::gpud::DISPLAY_*`; its IRQ
    // notification deliberately reuses the control-reply endpoint (never the server endpoint,
    // which would swallow windowd's present commands).
    reply_inbox: true,
    routes_to: &[Route {
        to: ServiceId::Socd,
        kind: RouteKind::ReplyInbox,
        slots: slots::gpud::SOCD,
    }],
    announce: true,
    server_slots: slots::gpud::SERVER,
    reply_slots: slots::gpud::REPLY,
    extra_slots: &[
        NamedSlotBinding { name: NamedSlot::TimerNotifyRecv, slot: slots::gpud::TIMER_RECV },
        NamedSlotBinding { name: NamedSlot::TimerNotifySend, slot: slots::gpud::TIMER_SEND },
        NamedSlotBinding { name: NamedSlot::DeviceWatchdogRecv, slot: slots::gpud::WATCHDOG.recv },
        NamedSlotBinding { name: NamedSlot::DeviceWatchdogSend, slot: slots::gpud::WATCHDOG.send },
        // RFC-0098 C7: the display-mode authority reads the lane's request from the tree.
        NamedSlotBinding { name: NamedSlot::DeviceTree, slot: slots::gpud::DEVICE_TREE },
    ],
};

/// The declaration of `inputd`.
pub(crate) const INPUTD: ServiceSpec = ServiceSpec {
    id: ServiceId::Inputd,
    stage: Stage::DisplayReady,
    exposes_server: true,
    // RFC-0098 C7: one call to windowd for the display space (gpud's mode) is answered on
    // this private inbox — windowd's shared response endpoint has several readers.
    reply_inbox: true,
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
    reply_slots: slots::inputd::REPLY,
    extra_slots: &[
        NamedSlotBinding { name: NamedSlot::Settings, slot: slots::inputd::SETTINGS_SEND },
        NamedSlotBinding { name: NamedSlot::SettingsWatchRecv, slot: slots::inputd::WATCH_RECV },
        NamedSlotBinding { name: NamedSlot::SettingsWatchSend, slot: slots::inputd::WATCH_SEND },
        NamedSlotBinding { name: NamedSlot::TimerNotifyRecv, slot: slots::inputd::TIMER_RECV },
        NamedSlotBinding { name: NamedSlot::TimerNotifySend, slot: slots::inputd::TIMER_SEND },
    ],
};

/// The declaration of `windowd`.
pub(crate) const WINDOWD: ServiceSpec = ServiceSpec {
    id: ServiceId::Windowd,
    stage: Stage::DisplayReady,
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
        NamedSlotBinding {
            name: NamedSlot::SessionWatchRecv,
            slot: slots::windowd::SESSION_WATCH_RECV,
        },
        NamedSlotBinding {
            name: NamedSlot::SessionWatchSend,
            slot: slots::windowd::SESSION_WATCH_SEND,
        },
    ],
};

// RFC-0075: imed's server pair is pre-minted; its windowd client leg is
// provisioned in the generic arm (fire-and-forget pushes, no reply inbox).
/// The declaration of `imed`.
pub(crate) const IMED: ServiceSpec = ServiceSpec {
    id: ServiceId::Imed,
    stage: Stage::DisplayReady,
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
    stage: Stage::DisplayReady,
    exposes_server: false,
    reply_inbox: false,
    routes_to: &[],
    announce: false,
    server_slots: SlotPair::UNDECLARED,
    reply_slots: SlotPair::UNDECLARED,
    extra_slots: &[],
};
