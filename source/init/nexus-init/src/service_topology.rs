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
//! This is the declarative service-topology manifest
//! for our chain: pure data, no syscall types, so it is validated on the
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
    /// Session manager daemon (RFC-0069 §4 — owns the `session-start` stage).
    Sessiond = 23,
    /// Typed settings registry daemon (TASK-0072 Phase 8 — persists prefs via statefsd).
    Settingsd = 24,
    /// System-internal compute broker (batch parallel work on nexus-workpool;
    /// invisible to apps — no app-facing route, system clients only).
    Pinched = 25,
    /// Input method editor daemon (RFC-0075 — composes keys for the focused
    /// surface; inputd forwards resolved keys, windowd relays focus).
    Imed = 26,
}

impl ServiceId {
    /// Number of entries needed to index a per-service array by `id as usize`
    /// (discriminants are `1..=26`, so the array spans `0..=26`; index 0 is unused).
    pub const COUNT: usize = 27;

    /// Every service identifier, for iterating a per-service routing array.
    pub const ALL: [ServiceId; 26] = [
        Self::Vfsd,
        Self::Packagefsd,
        Self::Policyd,
        Self::Bundlemgrd,
        Self::Updated,
        Self::Samgrd,
        Self::Execd,
        Self::Keystored,
        Self::Statefsd,
        Self::Rngd,
        Self::Timed,
        Self::Windowd,
        Self::Inputd,
        Self::Abilitymgr,
        Self::Gpud,
        Self::Netstackd,
        Self::Metricsd,
        Self::Logd,
        Self::Dsoftbusd,
        Self::Hidrawd,
        Self::Touchd,
        Self::SelftestClient,
        Self::Sessiond,
        Self::Settingsd,
        Self::Pinched,
        Self::Imed,
    ];

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
            b"sessiond" => Self::Sessiond,
            b"settingsd" => Self::Settingsd,
            b"pinched" => Self::Pinched,
            b"imed" => Self::Imed,
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
            Self::Sessiond => "sessiond",
            Self::Settingsd => "settingsd",
            Self::Pinched => "pinched",
            Self::Imed => "imed",
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
    (ServiceId::Abilitymgr, ServiceId::Sessiond),   // launch gate: session must be active
    // App-child service routing (TASK-0080C): execd resolves these BY NAME on
    // behalf of the app-hosts it spawns — one SEND clone per declared manifest
    // cap into the child's fixed SDK slot (`nexus-sdk-routes`).
    (ServiceId::Execd, ServiceId::Abilitymgr), // svc.ability.* (launcher e2e)
    (ServiceId::Execd, ServiceId::Bundlemgrd), // svc.bundle.* (app enumeration)
    (ServiceId::Execd, ServiceId::Sessiond),   // svc.session.* (DSL greeter login)
    (ServiceId::Execd, ServiceId::Settingsd),  // svc.settings.* (DSL settings app)
    (ServiceId::Execd, ServiceId::Vfsd),       // svc.files.* (filemanager, RFC-0073/TASK-0291)
    (ServiceId::Windowd, ServiceId::Bundlemgrd), // dynamic Apps menu (OP_LIST_APPS)
    (ServiceId::Windowd, ServiceId::Sessiond), // greeter/login relay (TASK-0065B)
    (ServiceId::Windowd, ServiceId::Settingsd), // theme GET/SET persistence (TASK-0072 P10)
    // RFC-0069 batches 1+2 (regular services migrated onto the declarative arm).
    (ServiceId::Rngd, ServiceId::Logd), // log sink (optional target)
    (ServiceId::Rngd, ServiceId::Policyd), // delegated policy checks
    (ServiceId::Vfsd, ServiceId::Packagefsd), // pkg:/ resolution (shared response ep)
    (ServiceId::Packagefsd, ServiceId::Bundlemgrd), // slot/manifest queries via CAP_MOVE
    (ServiceId::Samgrd, ServiceId::Logd), // structured logs via CAP_MOVE
    (ServiceId::Statefsd, ServiceId::Policyd), // policy checks via CAP_MOVE
    (ServiceId::Settingsd, ServiceId::Statefsd), // persist prefs (TASK-0072 Phase 8)
];

/// How a service receives the target's replies on a declared route (RFC-0069).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteKind {
    /// Send on the target's request endpoint; replies arrive on the caller's
    /// CAP_MOVE reply inbox (requires `reply_inbox`).
    ReplyInbox,
    /// Send on the target's request endpoint; replies arrive on the target's
    /// pre-minted RESPONSE endpoint, shared directly (no reply inbox).
    SharedResponse,
}

/// One declared outbound route (must appear in [`REQUIRED_ROUTES`]).
#[derive(Clone, Copy, Debug)]
pub struct Route {
    /// The callee.
    pub to: ServiceId,
    /// How replies come back.
    pub kind: RouteKind,
}

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
}

/// Declarative CPU placement SSOT (SMP soft-realtime plan P1). The display +
/// input chain is pinned to cpu0 (the soft-RT hart: its BKL competitors are
/// only each other), everything background runs on cpu1-3, so exec/vmo-heavy
/// bring-up work never steals cpu0 time from the interactive chain. Masks are
/// clamped by the kernel to ONLINE cpus, so SMP=1 degrades to cpu0 for all.
pub const fn affinity_for(name: &str) -> u8 {
    // const-fn string match via bytes (const_str_eq is not stable): compare
    // against the canonical names.
    const fn eq(a: &str, b: &str) -> bool {
        let (a, b) = (a.as_bytes(), b.as_bytes());
        if a.len() != b.len() {
            return false;
        }
        let mut i = 0;
        while i < a.len() {
            if a[i] != b[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    // Soft-realtime chain -> cpu0.
    if eq(name, "gpud")
        || eq(name, "windowd")
        || eq(name, "inputd")
        || eq(name, "hidrawd")
        || eq(name, "touchd")
        || eq(name, "imed")
    {
        return 0b0001;
    }
    // init itself + the selftest keep the full mask (the proof ladder tests
    // cross-cpu behaviour deliberately).
    if eq(name, "selftest-client") || eq(name, "init-lite") || eq(name, "nexus-init") {
        return 0b1111;
    }
    // Everything else is background -> cpu1-3 (kernel clamps to online).
    0b1110
}

/// The declared specs for services that participate in the v6b chain. Grown
/// incrementally; the host tests keep it consistent with `REQUIRED_ROUTES`.
pub const SERVICE_SPECS: &[ServiceSpec] = &[
    ServiceSpec {
        id: ServiceId::Abilitymgr,
        exposes_server: true,
        reply_inbox: true,
        routes_to: &[
            Route { to: ServiceId::Bundlemgrd, kind: RouteKind::ReplyInbox },
            Route { to: ServiceId::Execd, kind: RouteKind::ReplyInbox },
            Route { to: ServiceId::Sessiond, kind: RouteKind::ReplyInbox },
        ],
        announce: true,
    },
    ServiceSpec {
        id: ServiceId::Windowd,
        exposes_server: true,
        reply_inbox: true,
        routes_to: &[
            Route { to: ServiceId::Bundlemgrd, kind: RouteKind::ReplyInbox },
            Route { to: ServiceId::Sessiond, kind: RouteKind::ReplyInbox },
            Route { to: ServiceId::Settingsd, kind: RouteKind::ReplyInbox },
        ],
        announce: true,
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
            Route { to: ServiceId::Logd, kind: RouteKind::ReplyInbox },
            Route { to: ServiceId::Policyd, kind: RouteKind::ReplyInbox },
        ],
        announce: false,
    },
    ServiceSpec {
        id: ServiceId::Timed,
        exposes_server: true,
        reply_inbox: false,
        routes_to: &[],
        announce: false,
    },
    // RFC-0075: imed's server pair is pre-minted; its windowd client leg is
    // provisioned in the generic arm (fire-and-forget pushes, no reply inbox).
    ServiceSpec {
        id: ServiceId::Imed,
        exposes_server: true,
        reply_inbox: false,
        routes_to: &[],
        announce: false,
    },
    ServiceSpec {
        id: ServiceId::Vfsd,
        exposes_server: true,
        reply_inbox: false,
        routes_to: &[Route { to: ServiceId::Packagefsd, kind: RouteKind::SharedResponse }],
        announce: false,
    },
    ServiceSpec {
        id: ServiceId::Packagefsd,
        exposes_server: true,
        reply_inbox: true,
        routes_to: &[Route { to: ServiceId::Bundlemgrd, kind: RouteKind::ReplyInbox }],
        announce: false,
    },
    // Batch 3: their deleted arms printed the iw-gated `init: <svc> slots …`
    // line — announce=true keeps print + init_caps tally parity.
    ServiceSpec {
        id: ServiceId::Samgrd,
        exposes_server: true,
        reply_inbox: true,
        routes_to: &[Route { to: ServiceId::Logd, kind: RouteKind::ReplyInbox }],
        announce: true,
    },
    ServiceSpec {
        id: ServiceId::Statefsd,
        exposes_server: true,
        reply_inbox: true,
        routes_to: &[Route { to: ServiceId::Policyd, kind: RouteKind::ReplyInbox }],
        announce: true,
    },
    // Batch 4: pure server (optional pair — logd may be absent from an image).
    ServiceSpec {
        id: ServiceId::Logd,
        exposes_server: true,
        reply_inbox: false,
        routes_to: &[],
        announce: true,
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
    },
    // TASK-0072 Phase 8: the typed settings registry. Exposes a server (windowd
    // settings panel is a client, Phase 10) and calls statefsd to persist prefs
    // (its reply inbox = the shared `@reply` recipe). New service = this manifest
    // entry + its statefsd route + policy grant; the declarative arm wires it.
    ServiceSpec {
        id: ServiceId::Settingsd,
        exposes_server: true,
        reply_inbox: true,
        routes_to: &[Route { to: ServiceId::Statefsd, kind: RouteKind::ReplyInbox }],
        announce: false,
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
    #[test]
    fn affinity_ssot_masks_are_sane() {
        // Soft-RT chain on cpu0; nothing gets an empty mask.
        for d in ["gpud", "windowd", "inputd", "hidrawd", "touchd"] {
            assert_eq!(super::affinity_for(d), 0b0001, "{d}");
        }
        for s in super::ServiceId::ALL {
            assert_ne!(super::affinity_for(s.name()), 0, "{}", s.name());
        }
        assert_eq!(super::affinity_for("logd"), 0b1110);
    }

    use super::*;

    #[test]
    fn spec_lookup_is_declarative() {
        // The orchestrator's "does this service need a server endpoint?" decision
        // is data, host-tested — not a hand-written match arm.
        assert!(exposes_server(b"abilitymgr"));
        assert!(exposes_server(b"windowd"));
        assert!(!exposes_server(b"definitely-not-a-service"));
        let targets: alloc::vec::Vec<ServiceId> =
            spec_for(b"abilitymgr").unwrap().routes_to.iter().map(|r| r.to).collect();
        assert_eq!(targets, [ServiceId::Bundlemgrd, ServiceId::Execd, ServiceId::Sessiond]);
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
            for route in spec.routes_to {
                assert!(
                    REQUIRED_ROUTES.contains(&(spec.id, route.to)),
                    "spec {:?} routes_to {:?} but it is not in REQUIRED_ROUTES",
                    spec.id,
                    route.to
                );
            }
        }
    }

    /// A `ReplyInbox` route without a reply inbox can never receive its replies —
    /// catch the contradiction at test time, not as a silent runtime dead-end.
    #[test]
    fn reply_inbox_routes_require_reply_inbox() {
        for spec in SERVICE_SPECS {
            if spec.routes_to.iter().any(|r| r.kind == RouteKind::ReplyInbox) {
                assert!(spec.reply_inbox, "spec {:?} has ReplyInbox routes but no inbox", spec.id);
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
                    spec.routes_to.iter().any(|r| r.to == *to),
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
        let base =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../policies/base.toml");
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
                (l.starts_with(&format!("\"{name}\"")) || l.starts_with(&format!("{name} ")))
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
