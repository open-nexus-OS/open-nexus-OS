// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Service declaration for the proof harness (`selftest-client`) — the one consumer
//! that talks to nearly every service. Split from the per-plane spec files so the plane files
//! stay about the services themselves.
//! OWNERS: @runtime
//! STATUS: Production
//! API_STABILITY: Stable within the workspace
//! TEST_COVERAGE: crate tests in lib.rs + `nexus-init` route/policy cross-checks

use crate::routes::{Route, RouteKind};
use crate::specs::ServiceSpec;
use crate::Stage;
use crate::{slots, NamedSlot, NamedSlotBinding, ServiceId, SlotPair};

/// A route that answers on the target's shared response endpoint.
const fn shared(to: ServiceId, slots: SlotPair) -> Route {
    Route { to, kind: RouteKind::SharedResponse, slots }
}

/// A route that answers on the harness's CAP_MOVE reply inbox.
const fn inbox(to: ServiceId, slots: SlotPair) -> Route {
    Route { to, kind: RouteKind::ReplyInbox, slots }
}

/// The declaration of `selftest-client` (TASK-0324 P4f-5). Kinds follow each target's reply
/// discipline: netstackd, ingressd and virtioblkd answer on the caller's CAP_MOVE cap, so the
/// RECV halves the order-based arm granted there were never read and are not granted.
pub(crate) const SELFTEST_CLIENT: ServiceSpec = ServiceSpec {
    id: ServiceId::SelftestClient,
    stage: Stage::Platform,
    exposes_server: false,
    reply_inbox: true,
    routes_to: &[
        shared(ServiceId::Vfsd, slots::selftest_client::VFSD),
        shared(ServiceId::Packagefsd, slots::selftest_client::PACKAGEFSD),
        shared(ServiceId::Policyd, slots::selftest_client::POLICYD),
        shared(ServiceId::Bundlemgrd, slots::selftest_client::BUNDLEMGRD),
        shared(ServiceId::Updated, slots::selftest_client::UPDATED),
        shared(ServiceId::Samgrd, slots::selftest_client::SAMGRD),
        shared(ServiceId::Execd, slots::selftest_client::EXECD),
        shared(ServiceId::Keystored, slots::selftest_client::KEYSTORED),
        shared(ServiceId::Statefsd, slots::selftest_client::STATEFSD),
        shared(ServiceId::Logd, slots::selftest_client::LOGD),
        inbox(ServiceId::Inputd, slots::selftest_client::INPUTD),
        inbox(ServiceId::Netstackd, slots::selftest_client::NETSTACKD),
        shared(ServiceId::Dsoftbusd, slots::selftest_client::DSOFTBUSD),
        shared(ServiceId::Rngd, slots::selftest_client::RNGD),
        shared(ServiceId::Timed, slots::selftest_client::TIMED),
        shared(ServiceId::Metricsd, slots::selftest_client::METRICSD),
        shared(ServiceId::Pinched, slots::selftest_client::PINCHED),
        shared(ServiceId::Settingsd, slots::selftest_client::SETTINGSD),
        inbox(ServiceId::Ingressd, slots::selftest_client::INGRESSD),
        shared(ServiceId::Imed, slots::selftest_client::IMED),
        inbox(ServiceId::ImedOsk, slots::selftest_client::IMED_OSK),
        inbox(ServiceId::Bootctld, slots::selftest_client::BOOTCTLD),
        inbox(ServiceId::Virtioblkd, slots::selftest_client::VIRTIOBLKD),
    ],
    announce: true,
    server_slots: SlotPair::UNDECLARED,
    reply_slots: slots::selftest_client::REPLY,
    extra_slots: &[
        NamedSlotBinding { name: NamedSlot::FwCfg, slot: slots::selftest_client::FW_CFG },
        NamedSlotBinding {
            name: NamedSlot::TimerNotifyRecv,
            slot: slots::selftest_client::TIMER.recv,
        },
        NamedSlotBinding {
            name: NamedSlot::TimerNotifySend,
            slot: slots::selftest_client::TIMER.send,
        },
    ],
};
