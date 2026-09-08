// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: ingressd — the ONE inbound gateway (RFC-0092 Layer B, ADR-0061).
//! Services never bind a NIC-facing address; they declare
//! `[[expose."<subject>"]]` in the policy SSOT and register the intent here
//! by kernel identity. This crate is the pure core: the build-time exposure
//! table (`table`), the wire vocabulary (`wire`), the identity-bound intent
//! registry with policyd as authority (`intent`, `dispatch`), the accept-side
//! CIDR filter (`cidr`) and deterministic token bucket (`rate`), and the
//! bounded stream/datagram relays (`forward`, `udp`). No allocation, no
//! `std` (feature-gated host helpers only) — the OS-lite entry (P3) wraps
//! this core in the facade/policyd slots init wires.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests/ingress_host/ (allow + the five `test_reject_*`),
//!   tests/wire_contract.rs, unit tests per module
//! RFC: docs/rfcs/RFC-0092-service-exposure-contract-ingress-policy-ingressd.md
//! ADR: docs/adr/0061-exposure-intent-instead-of-free-binds.md

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
extern crate alloc;

pub mod cidr;
pub mod dispatch;
pub mod forward;
pub mod intent;
pub mod rate;
pub mod table;
pub mod udp;
pub mod wire;

/// OS-lite service (TASK-0052 P3): the gateway loop over init's fixed slots.
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub mod os_lite;

/// Host tooling: prints the build-time exposure table (one line per
/// exposure) — what the gateway WOULD front on this policy.
#[cfg(feature = "std")]
pub fn host_explain() -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let entries = table::generated::EXPOSE_ENTRIES;
    let _ = writeln!(out, "ingressd: {} exposure(s) compiled from policies/", entries.len());
    for e in entries {
        let _ = writeln!(
            out,
            "  subject=0x{:016x} {}/{} -> 127.0.0.1:{} cidrs={} rate={}/s burst={}",
            e.subject_id,
            e.proto.label(),
            e.port,
            e.backend,
            e.cidr_allow.len(),
            e.rate_per_s,
            e.burst
        );
    }
    out
}
