// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The SSOT for **declarative, cap-gated per-app service routing**.
//! An app's manifest declares `caps = […]`; the routing is DERIVED from this
//! table, not hand-wired per app. One entry ties together the three things
//! that otherwise drift apart:
//!   - the DSL service namespace (`svc.<name>.method`, from
//!     `tools/nexus-idl/schemas/dsl_services.capnp`),
//!   - the backing OS service the route resolves to (the responder name),
//!   - and the manifest permission (`nexus.permission.*`) that grants it.
//!
//! Both ends read THIS table: the launch authority (abilitymgr) provisions
//! exactly the routes whose permission the app was granted; the app-host
//! runtime maps `svc.<name>` → the fixed child slot the route landed in. Add a
//! service = one row here — no per-app code, no per-service special-casing.
//! OWNERS: @runtime
//! STATUS: Experimental (TASK-0080C — declarative app-child service routing)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: consistency guard below.

#![no_std]
#![forbid(unsafe_code)]

#[cfg(test)]
extern crate alloc;

/// One `svc.<name>` → OS route + permission + child-slot binding.
#[derive(Clone, Copy, Debug)]
pub struct ServiceRoute {
    /// DSL service namespace: `svc.<svc>.method` (matches `dsl_services.capnp`).
    pub svc: &'static str,
    /// The OS service the route resolves to (the responder route name).
    pub route: &'static str,
    /// The manifest permission that grants this route (fail-closed): an app
    /// only receives the route if it declared — and was granted — this cap.
    pub permission: &'static str,
    /// The FIXED child cap slot receiving the SEND half of this route. Fixed
    /// (not packed by grant order) so the app-host contract is stable
    /// regardless of which subset of routes an app was granted; an ungranted
    /// slot is simply empty and the runtime presence-probes it.
    pub child_slot: u32,
}

/// The child's shared reply inbox (RECV) — every service reply lands here;
/// requests carry a moved SEND clone of it so the service answers this slot.
pub const CHILD_REPLY_RECV_SLOT: u32 = nexus_service_topology::slots::app_child::REPLY.recv;
/// The SEND half of the reply inbox (the child clones + moves it per request).
pub const CHILD_REPLY_SEND_SLOT: u32 = nexus_service_topology::slots::app_child::REPLY.send;
/// First per-service SEND slot; `ServiceRoute::child_slot` values start here.
/// (Child slots 5/6 = windowd, 7 = payload, 8 = events are already taken.)
pub const CHILD_SVC_SLOT_BASE: u32 = nexus_service_topology::slots::app_child::SVC_BASE;

/// The curated routing table (SSOT). Add a service by adding a row.
pub const SERVICE_ROUTES: &[ServiceRoute] = &[
    ServiceRoute {
        svc: "bundlemgr",
        route: "bundlemgrd",
        permission: "nexus.permission.ENUMERATE",
        child_slot: 11,
    },
    ServiceRoute {
        svc: "ability",
        route: "abilitymgr",
        permission: "nexus.permission.LAUNCH",
        child_slot: 12,
    },
    ServiceRoute {
        svc: "session",
        route: "sessiond",
        permission: "nexus.permission.SESSION",
        child_slot: 13,
    },
    // Slot 14 is the app-host's EVENTS_SEND_CLONE_SLOT — routes skip it.
    ServiceRoute {
        svc: "settings",
        route: "settingsd",
        permission: "nexus.permission.SETTINGS",
        child_slot: 15,
    },
    // File surface (RFC-0073): routed directly to vfsd — per-app mediation is
    // vfsd's namespace layer (RFC-0042). FILES is ceiling-gated to the
    // `filemanager` bundle type at pack time (nxb-pack).
    ServiceRoute {
        svc: "files",
        route: "vfsd",
        permission: "nexus.permission.FILES",
        child_slot: 16,
    },
    // Wall-clock reads (RFC-0076): the app-host queries `timed`'s
    // OP_GET_WALLTIME to drive the minute clock tick; DSL-facing `svc.time.*`
    // methods land with a later slice (route provisioned now, host-driven).
    ServiceRoute {
        svc: "time",
        route: "timed",
        permission: "nexus.permission.TIME",
        child_slot: 17,
    },
    // OSK key injection (RFC-0075 Phase 2): the route resolves imed's
    // DEDICATED osk endpoint (`imed-osk`, not the main endpoint) —
    // possession of the SEND cap IS the injection authorization. IME is
    // ceiling-gated to the `ime` bundle type at pack time (nxb-pack).
    ServiceRoute {
        svc: "ime",
        route: "imed-osk",
        permission: "nexus.permission.IME",
        child_slot: 18,
    },
    // OTA status/actions (RFC-0089 §8, TASK-0140): routed to `updated` —
    // reads (status/feed/check) are open, mutating ops are gated INSIDE
    // updated on the policyd `updates.manage` grant (deny-by-default;
    // holding this route is deliberately NOT mutation authority). UPDATES
    // is ceiling-gated to the `settings` bundle type at pack time.
    // Child slot 22, not 19: 19 is the shared glyph atlas (`app_child::ATLAS_VMO`) and
    // 20/21 the timer pair — the grant into 19 failed on every boot
    // (`execd: FAIL app route grant svc=updates`, found 2026-10-07 by TASK-0067);
    // `test_reject_child_slot_collision` now checks every fixed child slot.
    ServiceRoute {
        svc: "updates",
        route: "updated",
        permission: "nexus.permission.UPDATES",
        child_slot: 22,
    },
    // Clipboard (TASK-0067, RFC-0094): text items in a bounded history. Holding the route
    // is the WRITE capability; reads are gated INSIDE clipboardd by the focus truth windowd
    // pushes (the focused window pastes; the shell and the keyboard browse the history).
    ServiceRoute {
        svc: "clipboard",
        route: "clipboardd",
        permission: "nexus.permission.CLIPBOARD",
        child_slot: 23,
    },
    // Screen capture (TASK-0068, RFC-0095): freeze, crop, save to the Pictures folder. The route
    // IS the capability — SCREENCAP is ceiling-gated to the `shell` and `settings` bundle types
    // at pack time (nxb-pack), so no app can read the screen.
    ServiceRoute {
        svc: "screencap",
        route: "screencapd",
        permission: "nexus.permission.SCREENCAP",
        child_slot: 24,
    },
];

/// The route for a DSL service namespace, if the platform backs it.
#[must_use]
pub fn route_for_svc(svc: &str) -> Option<&'static ServiceRoute> {
    SERVICE_ROUTES.iter().find(|r| r.svc == svc)
}

/// The route granted by a manifest permission, if any.
#[must_use]
pub fn route_for_permission(permission: &str) -> Option<&'static ServiceRoute> {
    SERVICE_ROUTES.iter().find(|r| r.permission == permission)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_internally_consistent() {
        for r in SERVICE_ROUTES {
            assert!(!r.svc.is_empty() && !r.route.is_empty());
            assert!(
                r.permission.starts_with("nexus.permission."),
                "route permission must be a platform permission: {}",
                r.permission
            );
            assert!(r.child_slot >= CHILD_SVC_SLOT_BASE, "service slot below base: {}", r.svc);
        }
        // No two routes may collide on svc name, permission, or child slot.
        for (i, a) in SERVICE_ROUTES.iter().enumerate() {
            for b in &SERVICE_ROUTES[i + 1..] {
                assert_ne!(a.svc, b.svc, "duplicate svc {}", a.svc);
                assert_ne!(a.permission, b.permission, "duplicate permission {}", a.permission);
                assert_ne!(a.child_slot, b.child_slot, "slot collision at {}", a.child_slot);
            }
        }
        // Reply-inbox slots must not collide with a service slot.
        for r in SERVICE_ROUTES {
            assert_ne!(r.child_slot, CHILD_REPLY_RECV_SLOT);
            assert_ne!(r.child_slot, CHILD_REPLY_SEND_SLOT);
        }
    }

    #[test]
    fn lookups_resolve_both_directions() {
        let r = route_for_svc("bundlemgr").expect("bundlemgr routed");
        assert_eq!(r.route, "bundlemgrd");
        assert_eq!(r.permission, "nexus.permission.ENUMERATE");
        assert_eq!(route_for_permission("nexus.permission.SESSION").unwrap().svc, "session");
        assert!(route_for_svc("library").is_none(), "example svc has no OS route");
    }
}

#[cfg(test)]
mod topology_agreement {
    use super::*;
    use nexus_service_topology::ServiceId;

    /// TASK-0324 P4 (RFC-0093 §4): the app-child view and the service topology are ONE truth.
    /// A `svc.*` row whose backing route is not a declared service is a route that init can
    /// never provision — today a silent "app has no such service", caught here at declaration
    /// level instead.
    #[test]
    fn test_reject_sdk_routes_diverge_from_topology() {
        for row in SERVICE_ROUTES {
            assert!(
                ServiceId::from_name(row.route.as_bytes()).is_some(),
                "svc.{} routes to '{}', which is not a service in nexus-service-topology",
                row.svc,
                row.route
            );
        }
    }

    /// Child slots are a per-app space: fixed, unique, and never colliding with the
    /// reserved windowd/payload/events/atlas/timer slots or the shared reply inbox —
    /// EVERY fixed `app_child` slot, not only the reply pair (the updates route sat on the
    /// atlas slot until 2026-10-07 because this list named two of them).
    #[test]
    fn test_reject_child_slot_collision() {
        use nexus_service_topology::slots::app_child;
        let reserved = [
            CHILD_REPLY_RECV_SLOT,
            CHILD_REPLY_SEND_SLOT,
            app_child::WINDOWD.send,
            app_child::WINDOWD.recv,
            app_child::PAYLOAD_VMO,
            app_child::EVENTS_RECV,
            app_child::EVENTS_SEND,
            app_child::ATLAS_VMO,
            app_child::TIMER_RECV,
            app_child::TIMER_SEND,
        ];
        let mut seen: alloc::vec::Vec<u32> = alloc::vec::Vec::new();
        for row in SERVICE_ROUTES {
            assert!(
                row.child_slot >= CHILD_SVC_SLOT_BASE,
                "svc.{} uses child slot {} below the service base",
                row.svc,
                row.child_slot
            );
            assert!(
                !reserved.contains(&row.child_slot),
                "svc.{} collides with a fixed app-child slot ({})",
                row.svc,
                row.child_slot
            );
            assert!(!seen.contains(&row.child_slot), "child slot {} claimed twice", row.child_slot);
            seen.push(row.child_slot);
        }
    }
}
