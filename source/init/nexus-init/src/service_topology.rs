// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Declarative service topology — host-testable SSOT for service
//! identity + the required route graph, cross-validated against policy (RFC-0066).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 6 tests
//!
//! Declarative service topology (RFC-0066) — the host-testable SSOT for *who
//! exists* and *who may talk to whom*, decoupled from the OS capability binding.
//!
//! This is the `.cml`-equivalent (Fuchsia) / samgr-catalog (OHOS) / launchd-plist
//! (Apple) for our chain: pure data, no syscall types, so it is validated on the
//! host. `route_table` (OS-only) binds these routes to capability slots; here we
//! only declare identity, the route graph, and per-service expectations — and a
//! host test cross-validates them so adding a service without its route/endpoint
//! is a `cargo test` failure, not a boot crash.

/// Compact service identifier — maps 1:1 to the canonical service name.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(u8)]
pub enum ServiceId {
    /// Virtual file system daemon.
    Vfsd = 1,
    /// Package file system daemon.
    Packagefsd = 2,
    /// Policy enforcement daemon.
    Policyd = 3,
    /// Bundle manager daemon.
    Bundlemgrd = 4,
    /// Update daemon.
    Updated = 5,
    /// System ability manager daemon.
    Samgrd = 6,
    /// Exec daemon.
    Execd = 7,
    /// Key store daemon.
    Keystored = 8,
    /// State file system daemon.
    Statefsd = 9,
    /// Random number generator daemon.
    Rngd = 10,
    /// Timer daemon.
    Timed = 11,
    /// Window manager / compositor daemon.
    Windowd = 12,
    /// Input routing daemon.
    Inputd = 13,
    /// Ability lifecycle manager daemon (RFC-0065).
    Abilitymgr = 14,
    /// GPU driver daemon.
    Gpud = 15,
    /// Network stack daemon.
    Netstackd = 16,
    /// Metrics daemon.
    Metricsd = 17,
    /// Log daemon.
    Logd = 18,
    /// Distributed soft bus daemon.
    Dsoftbusd = 19,
    /// HID raw input daemon.
    Hidrawd = 20,
    /// Touch input daemon.
    Touchd = 21,
    /// Selftest client.
    SelftestClient = 22,
}

impl ServiceId {
    /// Look up a service by its canonical name. Returns None for unknown names.
    pub fn from_name(name: &[u8]) -> Option<Self> {
        Some(match name {
            b"vfsd" => Self::Vfsd,
            b"packagefsd" => Self::Packagefsd,
            b"policyd" => Self::Policyd,
            b"bundlemgrd" => Self::Bundlemgrd,
            b"updated" => Self::Updated,
            b"samgrd" => Self::Samgrd,
            b"execd" => Self::Execd,
            b"keystored" => Self::Keystored,
            b"statefsd" => Self::Statefsd,
            b"rngd" => Self::Rngd,
            b"timed" => Self::Timed,
            b"windowd" => Self::Windowd,
            b"inputd" => Self::Inputd,
            b"abilitymgr" => Self::Abilitymgr,
            b"gpud" => Self::Gpud,
            b"netstackd" => Self::Netstackd,
            b"metricsd" => Self::Metricsd,
            b"logd" => Self::Logd,
            b"dsoftbusd" => Self::Dsoftbusd,
            b"hidrawd" => Self::Hidrawd,
            b"touchd" => Self::Touchd,
            b"selftest-client" => Self::SelftestClient,
            _ => return None,
        })
    }

    /// Returns the canonical service name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Vfsd => "vfsd",
            Self::Packagefsd => "packagefsd",
            Self::Policyd => "policyd",
            Self::Bundlemgrd => "bundlemgrd",
            Self::Updated => "updated",
            Self::Samgrd => "samgrd",
            Self::Execd => "execd",
            Self::Keystored => "keystored",
            Self::Statefsd => "statefsd",
            Self::Rngd => "rngd",
            Self::Timed => "timed",
            Self::Windowd => "windowd",
            Self::Inputd => "inputd",
            Self::Abilitymgr => "abilitymgr",
            Self::Gpud => "gpud",
            Self::Netstackd => "netstackd",
            Self::Metricsd => "metricsd",
            Self::Logd => "logd",
            Self::Dsoftbusd => "dsoftbusd",
            Self::Hidrawd => "hidrawd",
            Self::Touchd => "touchd",
            Self::SelftestClient => "selftest-client",
        }
    }
}

/// Declarative capability-route SSOT: the `(from → to)` service links the system
/// is expected to provision. Adding a service that needs a route without listing
/// it here (or vice versa) is caught by the host tests below.
pub const REQUIRED_ROUTES: &[(ServiceId, ServiceId)] = &[
    // App lifecycle / registry chain (RFC-0065).
    (ServiceId::Abilitymgr, ServiceId::Bundlemgrd), // resolve installed apps
    (ServiceId::Abilitymgr, ServiceId::Execd),      // spawn app processes
    (ServiceId::Windowd, ServiceId::Bundlemgrd),    // dynamic Apps menu (OP_LIST_APPS)
];

/// Per-service expectations the orchestrator must satisfy (RFC-0066). A service
/// that `exposes_server` must be given a server endpoint by init; `routes_to`
/// must each appear in [`REQUIRED_ROUTES`]. This is the declaration the
/// data-driven orchestrator (Phase 3) will consume to wire init generically.
#[derive(Clone, Copy, Debug)]
pub struct ServiceSpec {
    /// The service.
    pub id: ServiceId,
    /// Init must provision a server endpoint (recv/send slots) for it.
    pub exposes_server: bool,
    /// Init must provision a CAP_MOVE reply inbox for its outbound calls.
    pub reply_inbox: bool,
    /// Services it must be able to call (each must be in `REQUIRED_ROUTES`).
    pub routes_to: &'static [ServiceId],
}

/// The declared specs for services that participate in the v6b chain. Grown
/// incrementally; the host tests keep it consistent with `REQUIRED_ROUTES`.
pub const SERVICE_SPECS: &[ServiceSpec] = &[
    ServiceSpec {
        id: ServiceId::Abilitymgr,
        exposes_server: true,
        reply_inbox: true,
        routes_to: &[ServiceId::Bundlemgrd, ServiceId::Execd],
    },
    ServiceSpec {
        id: ServiceId::Windowd,
        exposes_server: true,
        reply_inbox: true,
        routes_to: &[ServiceId::Bundlemgrd],
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_lookup_is_declarative() {
        // The orchestrator's "does this service need a server endpoint?" decision
        // is data, host-tested — not a hand-written match arm.
        assert!(exposes_server(b"abilitymgr"));
        assert!(exposes_server(b"windowd"));
        assert!(!exposes_server(b"definitely-not-a-service"));
        assert_eq!(spec_for(b"abilitymgr").unwrap().routes_to, &[ServiceId::Bundlemgrd, ServiceId::Execd]);
    }

    #[test]
    fn names_round_trip() {
        for (from, to) in REQUIRED_ROUTES {
            assert_eq!(ServiceId::from_name(from.name().as_bytes()), Some(*from));
            assert_eq!(ServiceId::from_name(to.name().as_bytes()), Some(*to));
        }
        assert_eq!(ServiceId::from_name(b"nope"), None);
    }

    #[test]
    fn required_routes_are_distinct_and_non_self() {
        for (i, a) in REQUIRED_ROUTES.iter().enumerate() {
            assert_ne!(a.0, a.1, "self-route invalid: {a:?}");
            for b in &REQUIRED_ROUTES[i + 1..] {
                assert_ne!(a, b, "duplicate route {a:?}");
            }
        }
    }

    /// The two SSOTs must agree: every `ServiceSpec.routes_to` edge is a declared
    /// `REQUIRED_ROUTES` entry. This is the check that fails on the host when a
    /// service's route is added in one place but not the other.
    #[test]
    fn service_specs_match_required_routes() {
        for spec in SERVICE_SPECS {
            for &to in spec.routes_to {
                assert!(
                    REQUIRED_ROUTES.contains(&(spec.id, to)),
                    "spec {:?} routes_to {:?} but it is not in REQUIRED_ROUTES",
                    spec.id,
                    to
                );
            }
        }
    }

    /// Conversely, every declared route whose `from` has a spec must be covered by
    /// that spec's `routes_to` — so you cannot declare a route a service won't make.
    #[test]
    fn required_routes_covered_by_specs() {
        for (from, to) in REQUIRED_ROUTES {
            if let Some(spec) = SERVICE_SPECS.iter().find(|s| s.id == *from) {
                assert!(
                    spec.routes_to.contains(to),
                    "route {from:?}->{to:?} not covered by {from:?}'s spec"
                );
            }
        }
    }

    /// Cross-SSOT guard (RFC-0066, "better errors in future"): every service that
    /// needs to *route* (per `service_topology`) MUST be granted `ipc.core` in the
    /// policy SSOT (`policies/base.toml`) — otherwise the responder policy-denies its
    /// route at runtime and the caller sees a silent "unreachable" (the exact bug
    /// that cost a debug cycle: abilitymgr/windowd were missing from base.toml).
    /// This makes that omission a `cargo test` failure instead of a boot hunt.
    #[test]
    fn routing_services_are_granted_ipc_core_in_policy() {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../policies/base.toml");
        let toml = std::fs::read_to_string(&base)
            .unwrap_or_else(|e| panic!("read {}: {e}", base.display()));

        for spec in SERVICE_SPECS {
            if spec.routes_to.is_empty() {
                continue;
            }
            let name = spec.id.name();
            // Find the `[allow]` line for this service (quoted or bare key) and
            // require it to grant "ipc.core".
            let granted = toml.lines().any(|line| {
                let l = line.trim_start();
                (l.starts_with(&format!("\"{name}\""))
                    || l.starts_with(&format!("{name} ")))
                    && l.contains('=')
                    && l.contains("ipc.core")
            });
            assert!(
                granted,
                "service `{name}` routes ({:?}) but is not granted \"ipc.core\" in \
                 policies/base.toml — add it to the [allow] table or it will be \
                 policy-denied at boot (silent 'unreachable')",
                spec.routes_to
            );
        }
    }
}
