// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Typed capability routing table — the single source of truth for IPC routes
//! between services. Replaces the hardcoded match blocks previously in `os_payload.rs`.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: Unit tests in this module
//! ADR: docs/adr/0017-service-architecture.md
//!
//! PUBLIC API:
//!   - ServiceId: newtype identifier for a service (u8 discriminant)
//!   - CapSlot: typed capability slot with rights mask
//!   - RouteTable: central routing table with add/lookup/iter
//!
//! SECURITY INVARIANTS:
//!   - CapSlot rights are immutable after construction
//!   - Routes are scoped: a service can only see routes from itself
//!   - No raw u32 slot leakage outside this module
//!
//! ERROR CONDITIONS:
//!   - Unknown service name → RouteError::UnknownService
//!   - Route not found → RouteError::RouteNotFound

extern crate alloc;

use alloc::vec::Vec;
use nexus_abi::Rights;

/// Re-exported from the host-testable declarative topology (RFC-0066).
pub use crate::service_topology::{ServiceId, REQUIRED_ROUTES};

/// A typed capability slot with associated rights.
#[must_use = "capability slots must be explicitly used or closed"]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapSlot {
    /// Raw slot number in the capability table.
    pub slot: u32,
    /// Rights mask for this capability handle.
    pub rights: Rights,
}

impl CapSlot {
    /// Create a new capability slot descriptor.
    pub const fn new(slot: u32, rights: Rights) -> Self {
        Self { slot, rights }
    }
}

/// A route from a requester to a target service.
#[must_use = "route lookups must be handled explicitly"]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceRoute {
    /// Slot the requester uses to send to the target.
    pub send: CapSlot,
    /// Slot the requester uses to receive from the target.
    pub recv: CapSlot,
}

/// Errors produced by route table operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteError {
    /// The service name is not recognised.
    UnknownService,
    /// No route exists for the requested (from, to) pair.
    RouteNotFound,
    /// A route with the same (from, to) already exists.
    DuplicateRoute,
}

/// Central routing table — maps (requester, target) → ServiceRoute.
///
/// This is populated by init-lite during service wiring and queried by
/// the routing responder at runtime. Every route is scoped to the requester
/// so a service only sees routes it was explicitly granted.
#[derive(Default)]
pub struct RouteTable {
    routes: Vec<RouteEntry>,
}

/// Internal route storage entry.
struct RouteEntry {
    from: ServiceId,
    to: ServiceId,
    route: ServiceRoute,
}

impl RouteTable {
    /// Create an empty routing table with space for 64 routes.
    pub fn new() -> Self {
        Self { routes: Vec::with_capacity(64) }
    }

    /// Add a route from `from` to `to`. Overwrites if already present.
    pub fn add_route(&mut self, from: ServiceId, to: ServiceId, send: CapSlot, recv: CapSlot) {
        // Remove any existing entry for this pair.
        self.routes.retain(|e| !(e.from == from && e.to == to));
        self.routes.push(RouteEntry { from, to, route: ServiceRoute { send, recv } });
    }

    /// Look up the route for `from` → `to`.
    pub fn lookup(&self, from: ServiceId, to: ServiceId) -> Option<ServiceRoute> {
        self.routes.iter().find(|e| e.from == from && e.to == to).map(|e| e.route)
    }

    /// Look up a route using raw byte names (for the routing responder).
    pub fn lookup_by_name(
        &self,
        from_name: &[u8],
        to_name: &[u8],
    ) -> Result<ServiceRoute, RouteError> {
        let from = ServiceId::from_name(from_name).ok_or(RouteError::UnknownService)?;
        let to = ServiceId::from_name(to_name).ok_or(RouteError::UnknownService)?;
        self.lookup(from, to).ok_or(RouteError::RouteNotFound)
    }

    /// Iterate all routes from a given service.
    pub fn routes_from(
        &self,
        from: ServiceId,
    ) -> impl Iterator<Item = (ServiceId, ServiceRoute)> + '_ {
        self.routes.iter().filter(move |e| e.from == from).map(|e| (e.to, e.route))
    }

    /// Returns the number of routes in the table.
    pub fn len(&self) -> usize {
        self.routes.len()
    }

    /// Returns true if the table has no routes.
    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_id_roundtrip() {
        for id in &[ServiceId::Vfsd, ServiceId::Windowd, ServiceId::Gpud, ServiceId::Samgrd] {
            let name = id.name();
            let parsed = ServiceId::from_name(name.as_bytes());
            assert_eq!(parsed, Some(*id), "roundtrip failed for {name}");
        }
    }

    #[test]
    fn unknown_service_returns_none() {
        assert_eq!(ServiceId::from_name(b"nonexistent"), None);
    }

    #[test]
    fn add_and_lookup_route() {
        let mut table = RouteTable::new();
        let send = CapSlot::new(0x30, Rights::SEND);
        let recv = CapSlot::new(0x31, Rights::RECV);

        table.add_route(ServiceId::Gpud, ServiceId::Windowd, send, recv);

        let route = table.lookup(ServiceId::Gpud, ServiceId::Windowd);
        assert!(route.is_some());
        assert_eq!(route.unwrap().send.slot, 0x30);
        assert_eq!(route.unwrap().recv.slot, 0x31);
    }

    #[test]
    fn lookup_missing_route_returns_none() {
        let table = RouteTable::new();
        assert!(table.lookup(ServiceId::Gpud, ServiceId::Gpud).is_none());
    }

    #[test]
    fn lookup_by_name() {
        let mut table = RouteTable::new();
        table.add_route(
            ServiceId::Gpud,
            ServiceId::Windowd,
            CapSlot::new(0x30, Rights::SEND),
            CapSlot::new(0x31, Rights::RECV),
        );

        let route = table.lookup_by_name(b"gpud", b"windowd").expect("route should exist");
        assert_eq!(route.send.slot, 0x30);
        assert_eq!(route.recv.slot, 0x31);
    }

    #[test]
    fn overwrite_route() {
        let mut table = RouteTable::new();
        table.add_route(
            ServiceId::Gpud,
            ServiceId::Windowd,
            CapSlot::new(0x30, Rights::SEND),
            CapSlot::new(0x31, Rights::RECV),
        );
        table.add_route(
            ServiceId::Gpud,
            ServiceId::Windowd,
            CapSlot::new(0x40, Rights::SEND),
            CapSlot::new(0x41, Rights::RECV),
        );

        let route = table.lookup(ServiceId::Gpud, ServiceId::Windowd).expect("route should exist");
        assert_eq!(route.send.slot, 0x40);
    }

    #[test]
    fn routes_from_scoped() {
        let mut table = RouteTable::new();
        table.add_route(
            ServiceId::Gpud,
            ServiceId::Windowd,
            CapSlot::new(0x30, Rights::SEND),
            CapSlot::new(0x31, Rights::RECV),
        );
        table.add_route(
            ServiceId::Windowd,
            ServiceId::Gpud,
            CapSlot::new(0x40, Rights::SEND),
            CapSlot::new(0x41, Rights::RECV),
        );

        let from_gpud: Vec<_> = table.routes_from(ServiceId::Gpud).collect();
        assert_eq!(from_gpud.len(), 1);
        assert_eq!(from_gpud[0].0, ServiceId::Windowd);
    }
}
