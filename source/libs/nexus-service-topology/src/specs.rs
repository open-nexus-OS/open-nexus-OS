// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The declarations themselves — the required route graph and the per-service
//! spec (what init must provision, which slots carry it, what the service may call).
//! OWNERS: @runtime
//! STATUS: Production
//! API_STABILITY: Stable within the workspace
//! TEST_COVERAGE: crate tests in lib.rs + `nexus-init` route/policy cross-checks

use crate::routes::{Route, RouteKind};
use crate::{slots, NamedSlot, NamedSlotBinding, ServiceId, SlotPair};

/// Per-service expectations the orchestrator must satisfy (RFC-0066/0069). A
/// service that `exposes_server` must be given a server endpoint by init;
/// `routes_to` must each appear in [`REQUIRED_ROUTES`]. This is the declaration
/// the data-driven orchestrator consumes to wire init generically.
#[derive(Clone, Copy, Debug)]
pub struct ServiceSpec {
    /// The service.
    pub id: ServiceId,
    /// Init must provision a server endpoint (recv/send slots) for it.
    pub exposes_server: bool,
    /// Init must provision a CAP_MOVE reply inbox for its outbound calls.
    pub reply_inbox: bool,
    /// Services it must be able to call (each must be in `REQUIRED_ROUTES`).
    pub routes_to: &'static [Route],
    /// Emit the `init: <svc> slots …` / `route->… ok` wire markers. True only
    /// where the pre-migration bespoke arm printed them — migrated arms keep
    /// byte-identical boot logs (RFC-0069 migration discipline).
    pub announce: bool,
    /// The service's OWN server endpoint slots (the pair init transfers into it).
    pub server_slots: SlotPair,
    /// The shared CAP_MOVE reply inbox slots, when `reply_inbox` is set.
    pub reply_slots: SlotPair,
    /// Everything that is neither a server pair nor a route: device MMIO, IRQ notify,
    /// settings/watch channels, the stage fence.
    pub extra_slots: &'static [NamedSlotBinding],
}

/// The declared specs for services that participate in the v6b chain. Grown
/// incrementally; the host tests keep it consistent with `REQUIRED_ROUTES`.
pub const SERVICE_SPECS: &[ServiceSpec] = &[
    ServiceSpec {
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
    },
    ServiceSpec {
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
            Route {
                to: ServiceId::Updated,
                kind: RouteKind::ReplyInbox,
                slots: slots::execd::UPDATED,
            },
            Route {
                to: ServiceId::Statefsd,
                kind: RouteKind::SharedResponse,
                slots: slots::execd::STATEFSD,
            },
            Route {
                to: ServiceId::Policyd,
                kind: RouteKind::ReplyInbox,
                slots: slots::execd::POLICYD,
            },
        ],
        announce: true,
        server_slots: slots::execd::SERVER,
        reply_slots: slots::execd::REPLY,
        extra_slots: &[
            NamedSlotBinding {
                name: NamedSlot::ProbePingSend,
                slot: slots::execd::PROBE_PING.send,
            },
            NamedSlotBinding {
                name: NamedSlot::ProbePingRecv,
                slot: slots::execd::PROBE_PING.recv,
            },
            NamedSlotBinding {
                name: NamedSlot::ProbeReplySend,
                slot: slots::execd::PROBE_REPLY.send,
            },
            NamedSlotBinding {
                name: NamedSlot::ProbeReplyRecv,
                slot: slots::execd::PROBE_REPLY.recv,
            },
        ],
    },
    ServiceSpec {
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
    },
    ServiceSpec {
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
    },
    ServiceSpec {
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
            Route {
                to: ServiceId::Imed,
                kind: RouteKind::SharedResponse,
                slots: slots::inputd::IMED,
            },
        ],
        announce: true,
        server_slots: slots::inputd::SERVER,
        reply_slots: SlotPair::UNDECLARED,
        extra_slots: &[
            NamedSlotBinding { name: NamedSlot::Settings, slot: slots::inputd::SETTINGS_SEND },
            NamedSlotBinding {
                name: NamedSlot::SettingsWatchRecv,
                slot: slots::inputd::WATCH_RECV,
            },
            NamedSlotBinding {
                name: NamedSlot::SettingsWatchSend,
                slot: slots::inputd::WATCH_SEND,
            },
        ],
    },
    ServiceSpec {
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
            Route {
                to: ServiceId::Gpud,
                kind: RouteKind::SharedResponse,
                slots: slots::windowd::GPUD,
            },
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
            NamedSlotBinding {
                name: NamedSlot::SettingsWatchRecv,
                slot: slots::windowd::WATCH_RECV,
            },
            NamedSlotBinding {
                name: NamedSlot::SettingsWatchSend,
                slot: slots::windowd::WATCH_SEND,
            },
        ],
    },
    // RFC-0069 batches 1+2: regular services wired ENTIRELY from the spec (the
    // bespoke arms are deleted). Their server pair is PRE-MINTED (see
    // `Endpoints::server_pair`) — the generic arm transfers it instead of
    // creating a fresh endpoint; `announce: false` because the deleted arms
    // printed nothing.
    ServiceSpec {
        id: ServiceId::Rngd,
        exposes_server: true,
        reply_inbox: true,
        routes_to: &[
            Route { to: ServiceId::Logd, kind: RouteKind::ReplyInbox, slots: slots::rngd::LOGD },
            Route {
                to: ServiceId::Policyd,
                kind: RouteKind::ReplyInbox,
                slots: slots::rngd::POLICYD,
            },
        ],
        announce: false,
        server_slots: slots::rngd::SERVER,
        reply_slots: slots::rngd::REPLY,
        extra_slots: &[],
    },
    ServiceSpec {
        id: ServiceId::Timed,
        exposes_server: true,
        reply_inbox: false,
        routes_to: &[],
        announce: false,
        server_slots: slots::timed::SERVER,
        reply_slots: SlotPair::UNDECLARED,
        extra_slots: &[],
    },
    // RFC-0075: imed's server pair is pre-minted; its windowd client leg is
    // provisioned in the generic arm (fire-and-forget pushes, no reply inbox).
    ServiceSpec {
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
    },
    ServiceSpec {
        id: ServiceId::Vfsd,
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
    },
    ServiceSpec {
        id: ServiceId::Packagefsd,
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
    },
    // Batch 3: their deleted arms printed the iw-gated `init: <svc> slots …`
    // line — announce=true keeps print + init_caps tally parity.
    ServiceSpec {
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
    },
    ServiceSpec {
        id: ServiceId::Statefsd,
        exposes_server: true,
        reply_inbox: true,
        routes_to: &[Route {
            to: ServiceId::Policyd,
            kind: RouteKind::ReplyInbox,
            slots: slots::statefsd::POLICYD,
        }],
        announce: true,
        server_slots: slots::statefsd::SERVER,
        reply_slots: slots::statefsd::REPLY,
        extra_slots: &[],
    },
    // RFC-0092 (TASK-0052 P3): both legs are CAP_MOVE reply-inbox routes —
    // policyd answers the delegated `net.expose` check, netstackd answers
    // every facade RPC on the same inbox (the gateway drains one RPC at a
    // time, so the two never interleave).
    ServiceSpec {
        id: ServiceId::Ingressd,
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
        extra_slots: &[],
    },
    // TASK-0315: the block-plane owner. `reply_inbox` despite empty routes:
    // the inbox is the driver's IRQ notify endpoint (this service makes no
    // outbound calls, so the channel is exclusively the interrupt wake).
    ServiceSpec {
        id: ServiceId::Virtioblkd,
        exposes_server: true,
        reply_inbox: false,
        routes_to: &[],
        announce: true,
        server_slots: slots::virtioblkd::SERVER,
        reply_slots: SlotPair::UNDECLARED,
        extra_slots: &[NamedSlotBinding {
            name: NamedSlot::IrqNotify,
            slot: slots::virtioblkd::IRQ_NOTIFY,
        }],
    },
    // Batch 4 (amended by TASK-0049C): logd persists evidence-class records
    // to statefsd (spill txns via its CAP_MOVE reply inbox — never the
    // shared response queue).
    ServiceSpec {
        id: ServiceId::Logd,
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
    },
    // Batch S (RFC-0069 §4): the session manager — a NEW service that is
    // nothing but this manifest entry on the init side (the whole point of the
    // declarative arm). Owns the `session-start` stage; today it auto-starts
    // the default session. The greeter/login docks onto its server endpoint.
    ServiceSpec {
        id: ServiceId::Sessiond,
        exposes_server: true,
        reply_inbox: false,
        routes_to: &[],
        announce: true,
        server_slots: slots::sessiond::SERVER,
        reply_slots: SlotPair::UNDECLARED,
        extra_slots: &[],
    },
    // TASK-0072 Phase 8: the typed settings registry. Exposes a server (windowd
    // settings panel is a client, Phase 10) and calls statefsd to persist prefs
    // (its reply inbox = the shared `@reply` recipe). New service = this manifest
    // entry + its statefsd route + policy grant; the declarative arm wires it.
    ServiceSpec {
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
    },
    // pinched: system-internal compute broker (SMP track Phase D). Exposes a
    // server for system clients (selftest, SDK batch paths); calls nobody —
    // its parallelism is in-process threads on nexus-workpool, not IPC.
    // Deliberately NOT in nexus-sdk-routes: apps must never see it.
    ServiceSpec {
        id: ServiceId::Pinched,
        exposes_server: true,
        reply_inbox: false,
        routes_to: &[],
        announce: false,
        server_slots: slots::pinched::SERVER,
        reply_slots: SlotPair::UNDECLARED,
        extra_slots: &[],
    },
    // bootctld: single boot-state authority (TASK-0050, ADR-0055). Exposes a
    // server (updated/init/selftest are clients). BESPOKE-wired with FIXED
    // slots (inbox 5/6, statefsd send 7): its statefs attach must not
    // resolve routes — init calls the boot-attempt handshake before the
    // responder serves, so a responder-dependent attach would deadlock.
    ServiceSpec {
        id: ServiceId::Bootctld,
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
    },
];

/// Looks up the declared [`ServiceSpec`] for a service by name, if any. This is
/// what the (data-driven) orchestrator consults to decide what to provision —
/// instead of a bespoke `match` arm per service.
pub fn spec_for(name: &[u8]) -> Option<&'static ServiceSpec> {
    let id = ServiceId::from_name(name)?;
    SERVICE_SPECS.iter().find(|s| s.id == id)
}

/// `true` if init must provision a server endpoint for `name` (declarative).
pub fn exposes_server(name: &[u8]) -> bool {
    spec_for(name).is_some_and(|s| s.exposes_server)
}
