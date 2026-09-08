---
title: TASK-0052 Security v3 (Ingress): default-deny inbound policy + ingressd userspace gateway + service exposure contract
status: In Progress (P0 delivered 2026-09-08)
owner: @security
created: 2025-12-23
depends-on:
  - TASK-0046 # configd (Done — satisfied)
  - TASK-0047 # policyd / Policy-as-Code (Done — satisfied)
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Policy as Code: tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Config broker (certs/rates): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Networking substrate: tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - DSoftBus hardening (future): tasks/TASK-0030-dsoftbus-discovery-authz-hardening-mdns-ttl-acl-ratelimit.md
  - Sandboxing + egress rules: tasks/TASK-0043-security-v2-sandbox-quotas-egress-abi-audit.md
  - DevX CLI: tasks/TASK-0045-devx-nx-cli-v1.md
  - Testing contract: scripts/qemu-test.sh
---

## End-state rewrite 2026-09-03 (binding; supersedes older sections where they differ)

Verified repo reality (Explore 2026-09-03): `ingressd` does not exist; every bind in the system
goes through the netstackd facade (`handlers/listen.rs`, `handlers/udp/bind.rs`), which already
distinguishes loopback from real binds structurally but authorizes nothing and knows no sender;
`NetBind` matches port ranges only (address dimension missing — `source/libs/nexus-abi` is an
approval zone, covered by TASK-0028 P0); TLS in `no_std` userland is not viable early. The task
therefore splits into two layers with different prerequisites.

### Goal (end state)

Inbound is default-deny: no service binds a non-loopback address without a policy-checked
exposure intent, and every exposed port is fronted by ONE userspace ingress gateway (`ingressd`)
that enforces CIDR allow-lists and rate limits per exposure and forwards to loopback backends.
The TLS/mTLS termination seam is fixed in the contract and delivered when the network track
resumes — never a stub that prints `ok`.

### Non-goals

Kernel changes; raw-NIC subjects (documented boundary, same as TASK-0043); TLS/mTLS
implementation in this task (contract slot only); outbound policy (TASK-0043).

### Decisions

- **Layer A — policy + bind gate, no new service**: `ingress` domain in Policy-as-Code (default
  deny), `net.bind` profile with an address class (`loopback` default, `any` only with an exposure
  intent), evaluated by policyd at the netstackd facade using the identity plumbing from
  TASK-0043 P2. Deny ⇒ `STATUS_DENY` + `!cap-deny` marker + `AuditReason::IngressDenied`.
- **Layer B — `ingressd`**: RFC seed „Service Exposure Contract“: `ExposeIntent {service, port,
  proto, cidr_allow[], rate}` in the capnp IDL SSOT, register/unregister fail-closed against
  policyd, accept-side CIDR filter, deterministic token-bucket rate limits, forwarding to loopback
  backends; TLS/mTLS as a named contract slot. Proof runs in the single-VM profile over loopback +
  QEMU user-net (no peer needed).
- Identity: exposure intents are attributed by `sender_service_id`; a forged intent for another
  service is rejected (`test_reject_forged_intent_sender`).

### P0 delivered 2026-09-08 — contract seed

- `docs/rfcs/RFC-0092-service-exposure-contract-ingress-policy-ingressd.md`: `[[expose."<subject>"]]`
  schema (port/proto, ≤ 8 CIDRs, token-bucket `rate_per_s`/`burst`, `tls` slot, loopback `backend`;
  duplicates and `tls != none` are parse errors), Layer A rule (`address = "any"` only for the
  `ingressd` subject — `SchemaError::AnyBindNeedsGateway`), `ingressd` wire (`OP_EXPOSE`/`OP_UNEXPOSE`/
  `OP_EXPOSE_STATUS`, reasons `policy|identity|limit|tls`), accept-side CIDR + rate + bounded
  forwarding, capabilities (`net.expose`, gateway `policy.delegate`), failure model, markers, the
  five `test_reject_*` names, phases P1–P3, TLS/IPv6/DSoftBus open questions.
- `docs/adr/0061-exposure-intent-instead-of-free-binds.md` (Accepted), `tools/nexus-idl/schemas/
  ingress.capnp` (host SSOT: `ExposeIntent`, `ExposeResponse`), `docs/security/ingress.md`, RFC +
  ADR indexes.
- Proof: `just check` docs gates. Next: P1 (Layer A).

### Packages

- **P0 — Contract** (approval `docs/rfcs`): RFC seed + ADR „Exposure intent instead of free
  binds“; `ingress` policy domain shape; marker contract.
- **P1 — Layer A** (identity plumbing landed with TASK-0043 P2 on 2026-09-07: `FacadeContext.sender_service_id`, `STATUS_DENY`, `authz.rs` seam with the `net.bind` address class, `seam_admits`; this package adds the `ingress` policy domain, the `any`-address gate and its markers): `net.bind` address class in schema v2, facade evaluation
  at listen/bind, host `test_reject_nonloopback_bind_without_intent`, OS `SELFTEST: ingress deny
  ok`.
- **P2 — `ingressd` host**: new `source/services/ingressd/` (src/ + tests/), `tests/ingress_host/`:
  allow, `test_reject_intent_policy_denied`, `test_reject_cidr`, `test_reject_rate_exceeded`,
  `test_reject_forged_intent_sender`.
- **P3 — `ingressd` OS**: init wiring + policy grants, markers `ingressd: ready`, `ingressd: port
  open (port=…, proto=…)`, `ingressd: deny (reason=policy|cidr|rate)`, `SELFTEST: ingress allow
  ok`, `SELFTEST: ingress deny ok`, `SELFTEST: ingress rate ok`; `ingress_denies_total{subject}`.

### Touched paths (corrected)

`source/services/netstackd/`, `source/services/policyd/`, `userspace/policy/`,
`source/libs/nexus-abi/` (via 0028 P0, approval), `tools/nexus-idl/schemas/` (ExposeIntent),
`source/services/ingressd/` (new), `source/init/nexus-init/` (wiring), `policies/`,
`tests/ingress_host/`, `source/apps/selftest-client/`, `docs/security/ingress.md` (new),
`scripts/qemu-test.sh` + `tools/nx/chains/markers.txt` (approval, markers).

### Stop conditions (Definition of Done — replaces older DoD)

Host: the five `test_reject_*` above green. OS: `SELFTEST: ingress deny ok` (Layer A) and
`ingressd: ready` / `port open` / `deny` / `SELFTEST: ingress allow|deny|rate ok` (Layer B) gated
in headless/smp1; a non-loopback bind without intent is denied in the real facade. Docs +
CHANGELOG + board; TLS slot recorded as open in the RFC.


## Metadata correction 2026-08-14

- `depends-on` was empty; the real prerequisites are configd (TASK-0046) and policyd (TASK-0047) —
  **both Done**, so this task is unblocked on them.
- Touched paths corrected: `userspace/libs/nexus-abi/` does not exist — the crate lives at
  `source/libs/nexus-abi/`. `schemas/policy/` does not exist — policy rules live in `policies/`
  (e.g. `policies/nexus.policy.toml`, `policies/base.toml`) and the schema is
  `schemas/policy.config.schema.json`.
- **Scope alert (nexus-abi):** "deny non-loopback bind" needs a new **address dimension** on the
  `NetBind` matcher in nexus-abi — today it matches **port ranges only**
  (`source/libs/nexus-abi/src/abi_filter.rs`). `source/libs/**` is an approval-gated protection
  zone; plan that change explicitly and get approval before touching it.
- **RFC seed required** for the `ingressd` service contract (new service API + exposure-intent IDL)
  before building, per the repo workflow (new service API → RFC seed first).

## Context

We want inbound traffic to be **default-deny**. Services should not bind publicly by default; instead
they must declare an explicit exposure intent that is checked against policy and enforced by a single
userspace ingress gateway.

Kernel remains unchanged; therefore the enforcement boundary is **userspace**:

- enforce via “who gets to bind” and by funneling inbound connections through `ingressd`,
- plus defense-in-depth via `nexus-abi` filtering (deny non-loopback binds by default).

This task is intentionally **host-first** and **OS-gated** because it depends on networking substrate
and (optionally) DSoftBus stream forwarding.

## Goal

Deliver:

1. A Policy-as-Code domain `ingress` with schema and default-deny rules.
2. An exposure contract (IDL + intent registration) that services use to request public exposure.
3. `ingressd` userspace gateway:
   - binds allowed ports,
   - enforces CIDR allowlists + rate limits,
   - optional TLS/mTLS termination (host-first; OS gating),
   - forwards to internal backends (loopback proxy; DSoftBus forwarding optional).
4. `nexus-abi` guardrail: deny non-loopback binds unless allowed by policy/cap token.
5. Host tests proving allow/deny/rate/mTLS behavior deterministically.

## Non-Goals

- Kernel changes.
- A full L7 reverse proxy suite (keep it minimal; focus on correct default-deny and enforceable contract).
- Running TLS in OS builds before the crypto stack + key provisioning story is ready (gated).

## Constraints / invariants

- Default deny: if no ingress rule matches, inbound exposure must fail closed.
- No fake success markers; port-open marker only after bind succeeded and policy is attached.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Bounded memory:
  - cap number of exposed ports,
  - cap concurrent connections per port,
  - cap rate-limiter state.

## Red flags / decision points

- **RED (userspace-only boundary)**:
  - Without kernel mediation, the strongest enforcement is “don’t grant net-bind caps” + ABI guardrails.
  - If a process can directly access raw NIC or has a bypass capability, it can bypass ingressd.
  - This must be documented as a boundary assumption: inbound enforcement depends on cap distribution and ABI filter usage.
- **YELLOW (TLS/mTLS feasibility in OS)**:
  - OS userland is `no_std` in places; TLS stacks may not be viable early. Host-first is mandatory.
  - OS can start with TCP + policy + rate limiting; TLS gates can be enabled later.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ingress_host/`:

- policy allow case: open port → allowed client succeeds
- CIDR deny case: blocked source rejected (simulated)
- rate limiting: burst throttled and counters increment
- mTLS deny case: wrong client identity rejected (host-only)

### Proof (OS/QEMU) — gated

UART markers (once networking + policy + config are present):

- `ingressd: ready`
- `ingressd: port open (port=..., proto=...)`
- `ingressd: deny (reason=policy|cidr|sni|rate)`
- `SELFTEST: ingress allow ok`
- `SELFTEST: ingress deny ok`
- `SELFTEST: ingress rate ok`

## Touched paths (allowlist)

- `policies/` + `schemas/policy.config.schema.json` (extend: ingress domain)
- `source/services/ingressd/` (new; RFC seed required first)
- `source/services/execd/` (optional: exposure intent registration)
- `source/libs/nexus-abi/` (bind guardrail; approval-gated zone — needs address dimension on NetBind)
- `tools/nx/` (optional follow-up: `nx ingress` or `nx policy` integration)
- `tests/ingress_host/`
- `docs/security/ingress.md`

## Plan (small PRs)

1. **Policy domain + schema**
   - Add `ingress` rules: port/proto/cidr/tls/mtls/sni/rate.
   - Default deny.

2. **Exposure contract**
   - Define IDL for `ExposeIntent` + register/unregister.
   - Registration fails closed if policy denies.

3. **ingressd**
   - Host-first loopback proxy and policy enforcement.
   - Optional TLS/mTLS termination behind feature gates.
   - Markers: `ready`, `port open`, `deny`.

4. **ABI guardrail**
   - Deny non-loopback binds by default.
   - Allow only if process holds an explicit “ingress bind” capability/token granted by execd/ingressd.

5. **Tests + docs**
   - Host tests and docs for enforcement model and limitations.
