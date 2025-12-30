---
title: TASK-0052 Security v3 (Ingress): default-deny inbound policy + ingressd userspace gateway + service exposure contract
status: Draft
owner: @security
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Policy as Code: tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Config broker (certs/rates): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Networking substrate: tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - DSoftBus hardening (future): tasks/TASK-0030-dsoftbus-discovery-authz-hardening-mdns-ttl-acl-ratelimit.md
  - Sandboxing + egress rules: tasks/TASK-0043-security-v2-sandbox-quotas-egress-abi-audit.md
  - DevX CLI: tasks/TASK-0045-devx-nx-cli-v1.md
  - Testing contract: scripts/qemu-test.sh
---

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

- `policies/` + `schemas/policy/` (extend: ingress domain)
- `source/services/ingressd/` (new)
- `source/services/execd/` (optional: exposure intent registration)
- `userspace/libs/nexus-abi/` (bind guardrail)
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

