// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host tests of the declarative service topology SSOT (split out
//! of `service_topology.rs` for the module-size ratchet): affinity masks,
//! spec lookup, name round-trips, REQUIRED_ROUTES ↔ SERVICE_SPECS
//! consistency, reply-inbox rules, policy `ipc.core` coverage.
//! OWNERS: @runtime
//! STATUS: Experimental

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
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../policies/base.toml");
    let toml =
        std::fs::read_to_string(&base).unwrap_or_else(|e| panic!("read {}: {e}", base.display()));

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
