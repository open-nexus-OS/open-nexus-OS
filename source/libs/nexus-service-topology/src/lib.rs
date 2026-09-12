// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The declarative service topology (RFC-0066 / RFC-0069 / RFC-0093 §4) — service
//! ids, required routes, per-service expectations AND the capability slots that carry them.
//! ONE home read by init (which provisions the slots), by every service (which compiles
//! against the same constants) and by the app-child view in `nexus-sdk-routes`. Before
//! TASK-0324 P4 the same numbers lived three times: a bespoke init arm, a `const … SLOT`
//! in the consumer, and a comment describing the order — a service wired in one place and
//! not the other was a boot crash, not a compile error.
//!
//! Slots were migrated ONE CONSUMER PER PACKAGE (P4a-P4f). Since P4f-4 every declared service
//! is complete: `SlotPair::UNDECLARED` only marks what a service does not have (no server, no
//! inbox), and `test_reject_partial_slot_declaration` fails any spec whose server, inbox or
//! route is left undeclared — the atomicity rule enforced mechanically instead of by review.
//!
//! OWNERS: @runtime
//! STATUS: Production (TASK-0324 P4)
//! API_STABILITY: Stable within the workspace
//! TEST_COVERAGE: Unit tests below + `nexus-init` topology tests (policy/route cross-checks)

#![no_std]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

/// A capability slot pair as seen by the RECEIVING task.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SlotPair {
    /// Slot holding the SEND half.
    pub send: u32,
    /// Slot holding the RECV half.
    pub recv: u32,
}

impl SlotPair {
    /// Not yet migrated onto the declared arm (slot 0 is never a real endpoint slot).
    pub const UNDECLARED: Self = Self { send: 0, recv: 0 };

    /// A declared pair.
    pub const fn new(send: u32, recv: u32) -> Self {
        Self { send, recv }
    }

    /// Whether both halves are declared.
    pub const fn is_declared(&self) -> bool {
        self.send != 0 && self.recv != 0
    }
}

/// The control channel every child receives from init at fixed slots: `@reply`,
/// `@mint-pair`, route asks and the `@ready` announce all travel here (RFC-0093 §1/§2).
pub const CTRL_SLOTS: SlotPair = SlotPair::new(1, 2);

/// Where a service's OWN server endpoint lands: RECV at 3, SEND at 4 — the first two slots
/// after the control channel. A fleet convention, declared once (TASK-0324 P4f-1a): it used to
/// be `SlotPair::new(4, 3)` in every service module and a literal `new_with_slots(3, 4)`
/// fallback in every service that asked init for its own slots.
pub const SERVER_SLOTS: SlotPair = SlotPair::new(4, 3);

/// The slot every device MMIO window lands in. One convention for the whole fleet: init
/// grants at this slot and each driver maps from it — the number used to live once per
/// driver plus once in init, seven copies that had to agree by hand.
pub const DEVICE_MMIO_SLOT: u32 = 48;

/// The band for capabilities init grants a service AFTER it has started running (TASK-0324
/// P4f-3). The core plane resumes virtioblkd, policyd and bundlemgrd before the wiring phase so
/// the system volume can be served; a running service allocates its own capabilities at the
/// lowest free slots, so a late grant declared low races those allocations — observed for
/// virtioblkd in P4f-1b. Late grants to such a service live from here up, above anything a
/// service allocates itself. (policyd's audit inbox 0x9-0xB predates the band and is safe for a
/// stated reason: its slots 1-8 are all pinned before it resumes and it allocates nothing
/// before it is wired.)
pub const LATE_GRANT_BASE: u32 = 0xE0;

/// The boot-stage fence (ADR-0062, RFC-0093 §3), pinned into EVERY child at spawn time with
/// `Rights::WAIT` alone: a child may block for a stage, never release one for the fleet. One
/// fleet-wide slot, not a per-service declaration — every service holds the same fence.
pub const STAGE_FENCE_SLOT: u32 = 0x38;

/// The block plane's client slots (TASK-0315 wiring, TASK-0324 P4f-1b home): every
/// block-plane client (statefsd, vfsd, bootctld, updated, bundlemgrd) receives virtioblkd's
/// request SEND at [`BLK_PLANE_REQ_SLOT`] and a private reply pair at [`BLK_PLANE_REPLY`],
/// in the same place in every table. The numbers lived in init's wiring and again in
/// `storage::blockproto`, whose comment promised "same numbers in every client's table".
pub const BLK_PLANE_REQ_SLOT: u32 = 0xF0;
/// The block plane's client reply pair (RECV 0xF1, SEND 0xF2).
pub const BLK_PLANE_REPLY: SlotPair = SlotPair::new(0xF2, 0xF1);

/// The virtio-input MMIO window slots (keyboard, pointer, tablet). Declared once for the
/// grant side (init) and the mapping side (hidrawd) — the block used to be a base constant
/// in init and a literal array in hidrawd that had to agree by hand.
pub const INPUT_MMIO_SLOTS: [u32; 3] = [50, 51, 52];

/// A capability a service receives that is neither its server pair nor a route.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NamedSlot {
    /// Device MMIO window (driver services).
    Mmio,
    /// IRQ notification endpoint (driver services).
    IrqNotify,
    /// Settings push channel (RFC-0083 watch): the RECV half, drained per frame.
    SettingsWatchRecv,
    /// Settings push channel: the SEND half, cloned per `OP_WATCH` registration.
    SettingsWatchSend,
    /// Settings request channel.
    Settings,
    /// Boot-stage fence, WAIT rights only (ADR-0062).
    StageFence,
    /// A second server endpoint's RECV half (imed's on-screen-keyboard endpoint, which
    /// answers through the service's own server SEND half).
    OskServerRecv,
    /// policyd: RECV half of init's private route-check channel (is requester X allowed to
    /// route to Y).
    PolicyRouteCheckRecv,
    /// policyd: SEND half of the route-check channel (the verdict back to init).
    PolicyRouteCheckSend,
    /// policyd: RECV half of init's private exec-check channel.
    PolicyExecCheckRecv,
    /// policyd: SEND half of the exec-check channel.
    PolicyExecCheckSend,
    /// recv-wake probe (execd): SEND half of the ping endpoint.
    ProbePingSend,
    /// recv-wake probe (execd): RECV half of the ping endpoint.
    ProbePingRecv,
    /// recv-wake probe (execd): SEND half of the reply endpoint.
    ProbeReplySend,
    /// recv-wake probe (execd): RECV half of the reply endpoint.
    ProbeReplyRecv,
    /// The QEMU firmware-config MMIO window (the proof harness's boot profile channel).
    FwCfg,
}

/// One named slot binding of a service.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NamedSlotBinding {
    /// What the slot carries.
    pub name: NamedSlot,
    /// The slot number in the service's capability table.
    pub slot: u32,
}

/// Service identity.
mod ids;
/// The route graph (required links, delivery kinds).
mod routes;
/// Per-service slot constants (split out of `specs` under the module-size ratchet).
pub mod slots;
/// Per-service declarations.
mod specs;
/// Declarations: the app platform.
mod specs_app;
/// Declarations: the proof harness.
mod specs_harness;
/// Declarations: the policy authority, entropy, the network edge.
mod specs_security;
/// Declarations: storage and boot.
mod specs_storage;
/// Declarations: the display and input chain.
mod specs_ui;
/// Boot stages: the monotone fence ladder every service is declared against.
pub mod stage;

pub use ids::ServiceId;
pub use routes::{Route, RouteKind, REQUIRED_ROUTES};
pub use specs::{exposes_server, spec_for, ServiceSpec, SERVICE_SPECS};
pub use stage::Stage;

/// The declared slots for the route `from` → `to`, if the pair is declared.
#[must_use]
pub fn route_slots(from: ServiceId, to: ServiceId) -> Option<SlotPair> {
    let spec = SERVICE_SPECS.iter().find(|s| s.id == from)?;
    let route = spec.routes_to.iter().find(|r| r.to == to)?;
    route.slots.is_declared().then_some(route.slots)
}

/// The declared route `from` → `to`, if its slots are declared (the kind carries what a
/// `PrivateInbox` route needs beyond the pair).
#[must_use]
pub fn declared_route(from: ServiceId, to: ServiceId) -> Option<Route> {
    let spec = SERVICE_SPECS.iter().find(|s| s.id == from)?;
    let route = spec.routes_to.iter().find(|r| r.to == to)?;
    route.slots.is_declared().then_some(*route)
}

/// `true` if `(send, recv)` is exactly the declared route `from` → `to`. The proof harness checks
/// init's routing answers with it (TASK-0324 P4f-5): a responder that hands out anything but the
/// declaration fails the routing proof instead of passing it because a client object could be
/// constructed.
#[must_use]
pub fn route_matches(from: ServiceId, to: ServiceId, send: u32, recv: u32) -> bool {
    route_slots(from, to).is_some_and(|slots| slots.send == send && slots.recv == recv)
}

/// The declared slot for a named capability of `svc`.
#[must_use]
pub fn extra_slot(svc: ServiceId, name: NamedSlot) -> Option<u32> {
    let spec = SERVICE_SPECS.iter().find(|s| s.id == svc)?;
    spec.extra_slots.iter().find(|b| b.name == name).map(|b| b.slot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    extern crate alloc;

    #[test]
    fn every_spec_id_is_unique() {
        let mut seen: Vec<ServiceId> = Vec::new();
        for spec in SERVICE_SPECS {
            assert!(!seen.contains(&spec.id), "duplicate spec for {:?}", spec.id);
            seen.push(spec.id);
        }
    }

    #[test]
    fn test_reject_partial_slot_declaration() {
        // A consumer is migrated ATOMICALLY (init arm + the consumer in one package). A spec
        // with some slots undeclared is exactly the half-migrated state that leaves a service
        // talking to slots nobody provisioned. Since P4f-4 there is no unmigrated spec left, so
        // the old "not migrated yet" exemption is gone: init's order-based transfer it
        // protected was deleted with it.
        for spec in SERVICE_SPECS {
            if spec.exposes_server {
                assert!(
                    spec.server_slots.is_declared(),
                    "{:?} is migrated but its server pair is undeclared",
                    spec.id
                );
            }
            if spec.reply_inbox {
                assert!(
                    spec.reply_slots.is_declared(),
                    "{:?} is migrated but its reply inbox is undeclared",
                    spec.id
                );
            }
            for route in spec.routes_to {
                assert!(
                    route.slots.is_declared(),
                    "{:?} is migrated but its route to {:?} is undeclared",
                    spec.id,
                    route.to
                );
            }
        }
    }

    #[test]
    fn test_reject_slot_collision_per_service() {
        for spec in SERVICE_SPECS {
            let mut used: Vec<u32> = Vec::new();
            let mut claim = |slot: u32, what: &str| {
                if slot == 0 {
                    return;
                }
                assert!(
                    !used.contains(&slot),
                    "{:?}: slot {slot} claimed twice (second claim: {what})",
                    spec.id
                );
                used.push(slot);
            };
            claim(spec.server_slots.send, "server send");
            claim(spec.server_slots.recv, "server recv");
            claim(spec.reply_slots.send, "reply send");
            claim(spec.reply_slots.recv, "reply recv");
            for route in spec.routes_to {
                claim(route.slots.send, "route send");
                // A ReplyInbox route answers on the service's ONE shared inbox by
                // definition — that slot is claimed once, above. Pointing such a route at
                // a different slot is the real defect, so assert the identity instead.
                if let RouteKind::PrivateInbox { inbox_send } = route.kind {
                    claim(route.slots.recv, "private inbox recv");
                    claim(inbox_send, "private inbox send");
                } else if route.kind == RouteKind::ReplyInbox {
                    if route.slots.is_declared() {
                        assert_eq!(
                            route.slots.recv, spec.reply_slots.recv,
                            "{:?}: ReplyInbox route to {:?} must answer on the shared inbox",
                            spec.id, route.to
                        );
                    }
                } else {
                    claim(route.slots.recv, "route recv");
                }
            }
            for binding in spec.extra_slots {
                claim(binding.slot, "named slot");
            }
        }
    }

    #[test]
    fn test_reject_slot_in_reserved_range() {
        // Fleet-wide slots are granted to EVERY service (the control channel) or to a whole
        // class of them (device MMIO, the input windows). A per-service declaration that lands
        // on one of them would be pinned into an occupied slot — the kernel refuses that
        // (`set_if_empty`), so the route would be dead at boot. Refuse it at compile-test time.
        let mut reserved: Vec<u32> = Vec::new();
        reserved.extend([CTRL_SLOTS.send, CTRL_SLOTS.recv, DEVICE_MMIO_SLOT, STAGE_FENCE_SLOT]);
        reserved.extend(INPUT_MMIO_SLOTS);
        reserved.extend([BLK_PLANE_REQ_SLOT, BLK_PLANE_REPLY.recv, BLK_PLANE_REPLY.send]);
        for spec in SERVICE_SPECS {
            let mut declared: Vec<(u32, &str)> = Vec::new();
            declared.push((spec.server_slots.send, "server send"));
            declared.push((spec.server_slots.recv, "server recv"));
            declared.push((spec.reply_slots.send, "reply send"));
            declared.push((spec.reply_slots.recv, "reply recv"));
            for route in spec.routes_to {
                declared.push((route.slots.send, "route send"));
                declared.push((route.slots.recv, "route recv"));
                if let RouteKind::PrivateInbox { inbox_send } = route.kind {
                    declared.push((inbox_send, "private inbox send"));
                }
            }
            for binding in spec.extra_slots {
                declared.push((binding.slot, "named slot"));
            }
            for (slot, what) in declared {
                assert!(
                    slot == 0 || !reserved.contains(&slot),
                    "{:?}: {what} declared in the fleet-reserved slot {slot}",
                    spec.id
                );
            }
        }
    }

    #[test]
    fn test_reject_service_missing_from_specs() {
        // Every process of the topology is declared — also one that holds nothing beyond the
        // control channel: an undeclared service is invisible to every slot test here.
        // `imed-osk` is not a process; it names imed's second server endpoint as a route target.
        for id in ServiceId::ALL {
            if id == ServiceId::ImedOsk {
                continue;
            }
            assert!(SERVICE_SPECS.iter().any(|spec| spec.id == id), "{id:?} has no ServiceSpec");
        }
    }

    #[test]
    fn test_reject_route_answer_diverging_from_declaration() {
        for spec in SERVICE_SPECS {
            for route in spec.routes_to {
                let slots = route.slots;
                assert!(route_matches(spec.id, route.to, slots.send, slots.recv));
                // A swapped pair, a shifted slot and a foreign slot are all divergence.
                if slots.send != slots.recv {
                    assert!(!route_matches(spec.id, route.to, slots.recv, slots.send));
                }
                assert!(!route_matches(spec.id, route.to, slots.send + 1, slots.recv));
                assert!(!route_matches(spec.id, route.to, slots.send, 0));
            }
        }
        // An undeclared route matches nothing, not even an all-zero answer.
        assert!(!route_matches(ServiceId::SelftestClient, ServiceId::Windowd, 0, 0));
        assert!(!route_matches(ServiceId::Touchd, ServiceId::Logd, 3, 4));
    }

    #[test]
    fn declared_slots_are_reachable_through_the_accessors() {
        for spec in SERVICE_SPECS {
            for route in spec.routes_to {
                assert_eq!(route_slots(spec.id, route.to).is_some(), route.slots.is_declared());
            }
            for binding in spec.extra_slots {
                assert_eq!(extra_slot(spec.id, binding.name), Some(binding.slot));
            }
        }
    }
}
