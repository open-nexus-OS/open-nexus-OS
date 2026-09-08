<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0061: Inbound exposure is an intent through ONE gateway — no service binds a non-loopback address itself

- Status: Accepted
- Date: 2026-09-08
- Links:
  - Tasks: `tasks/TASK-0052-security-v3-ingress-policy-ingressd-gateway.md` (execution + proof)
  - RFCs: `docs/rfcs/RFC-0092-service-exposure-contract-ingress-policy-ingressd.md` (the contract this decision fixes), `docs/rfcs/RFC-0091-policy-profile-v2-schema-wire-argument-matchers.md` (the `net.bind` address class and the facade seam it relies on)
  - Related ADRs: `docs/adr/0014-policy-architecture.md` (policyd is the single policy authority), `docs/adr/0057-service-restart-capability-re-resolve.md` (the gateway restarts like any service), `docs/adr/0051-nexus-wire-declarative-codec.md` (wire form of the gateway ops)

## Context

Every bind goes through netstackd's facade, which (TASK-0043 P2) knows the kernel-attributed
sender and asks policyd with the RFC-0091 `net.bind` address class. That makes a *policy* for
inbound possible but does not decide *who* may face the NIC. If every service may declare
`address = "any"` in its own profile, exposure review scatters over N profiles, nothing fronts
an exposed port (no CIDR allow-list, no rate limit, no place for TLS), and every exposed service
becomes NIC-facing code. The alternative — a firewall in the kernel — contradicts the
microkernel split (policy authority lives in userspace, the kernel has no network stack).

## Decision

- **Only the gateway binds a non-loopback address.** The shared policy grammar accepts
  `[[net.bind]] address = "any"` solely for the `ingressd` subject; every other subject binds
  loopback-only and a non-loopback bind is refused at the facade (`STATUS_DENY`,
  `ingress-denied`).
- **Exposure is a declared intent, not a bind.** A service that must be reachable declares
  `[[expose."<subject>"]] {port, proto, cidr_allow, rate_per_s, burst, tls, backend}` in the
  policy SSOT and registers the intent with `ingressd` at runtime; the gateway checks the
  kernel-attributed sender against the declared subject and policyd against `net.expose`,
  binds the NIC-facing port itself, filters peers by CIDR, rate-limits with a deterministic
  token bucket, and forwards to the subject's loopback backend.
- **The TLS/mTLS termination seam belongs to the gateway** and is a named slot in the
  contract; until it is delivered the grammar rejects `tls != "none"` — never a stub that
  reports `ok`.
- Out of scope: outbound policy (RFC-0091 `net.connect`), raw-NIC subjects (the
  `device.mmio.net` grant is the boundary), DSoftBus session authorization.

## Consequences

- **Positive**: one reviewable list of everything reachable from outside; one enforcement
  point for CIDR/rate/TLS; exposed services stay loopback-only code; refusals land in the one
  deny taxonomy and counter (`ingress-denied`, `ingress_denies_total{subject}`).
- **Negative / accepted cost**: one extra hop per inbound connection (loopback forward);
  services that bound `any` today must declare exposures (today: none besides the proofs);
  the gateway is a new service with init wiring and its own restart discipline.
- **Follow-ups**: TASK-0052 P1 (bind gate + `SELFTEST: ingress deny ok`), P2 (`ingressd`
  host + `tests/ingress_host/`), P3 (OS wiring + markers); TLS slot with the network track;
  DSoftBus ports (TASK-0030) decided when the network family resumes.

## Alternatives considered

- Per-service `any` binds guarded by policy only — rejected (no accept-side filters, N
  enforcement points, exposed services become NIC-facing).
- Kernel firewall — rejected (policy authority is userspace, the kernel has no stack).
- Gateway inside netstackd — rejected (the stack is not the policy actor; the facade's hot
  loop must stay free of accept-side policy; independent restart).
