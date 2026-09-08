<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# RFC-0092: Service Exposure Contract — default-deny inbound policy, exposure intents, and the `ingressd` gateway

- Status: Draft (Phase 0 seed 2026-09-08 — execution TASK-0052)
- Owners: @security @runtime
- Created: 2026-09-08
- Last Updated: 2026-09-08
- Links:
  - Tasks: `tasks/TASK-0052-security-v3-ingress-policy-ingressd-gateway.md` (execution + proof, P0–P3)
  - ADR: `docs/adr/0061-exposure-intent-instead-of-free-binds.md` (the one decision this contract rests on)
  - RFCs: `docs/rfcs/RFC-0091-policy-profile-v2-schema-wire-argument-matchers.md` (`net.bind` address class, `OP_ABI_EVAL`, deny taxonomy), `docs/rfcs/RFC-0068-structured-event-observability-subject-grouped-journal-renderer.md` (audit record)
  - Docs: `docs/security/network-egress.md` (the outbound half), `docs/security/abi-filters.md` (seams), `docs/security/ingress.md` (this contract's user-facing notes)
  - IDL: `tools/nexus-idl/schemas/ingress.capnp` (host SSOT of the exposure intent)

## Status at a Glance

- **Phase 0 (contract seed: exposure schema, bind gate rule, ingressd wire, markers)**: ✅ (2026-09-08, TASK-0052 P0)
- **Phase 1 (Layer A — bind gate: `any` only for the gateway; `SELFTEST: ingress deny ok`)**: ✅ (2026-09-08, TASK-0052 P1)
- **Phase 2 (Layer B — `ingressd` host: intents, CIDR accept filter, token bucket, forwarding)**: ⬜ (TASK-0052 P2)
- **Phase 3 (Layer B — `ingressd` OS: init wiring, markers, `SELFTEST: ingress allow|deny|rate ok`)**: ⬜ (TASK-0052 P3)
- **TLS / mTLS termination slot**: ⬜ reserved — delivered by the network track (never a stub)

Definition:

- "Complete" means the **contract** is defined and the **proof gates** are green (tests/markers). It does not mean "never changes again".

## Scope boundaries (anti-drift)

- **This RFC owns**: the `expose` policy domain (schema, precedence, bounds), the rule that only the gateway may bind a non-loopback address, the `ingressd` service contract (wire ops, identity, accept-side filters, rate limits, forwarding, markers, counters), and the named TLS/mTLS slot.
- **This RFC does NOT own**: outbound policy (RFC-0091 `net.connect`, `docs/security/network-egress.md`); the matcher/precedence engine and the `net.bind` address class themselves (RFC-0091); DSoftBus discovery/session authorization (TASK-0030); TLS implementation (network track); IPv6 (reserved, like RFC-0091).

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define **stop conditions** and **proof commands**.
- TASK-0052 P0 seeds this contract; P1 proves Layer A, P2/P3 deliver and prove Layer B. Markers below are the three-way SSOT with `scripts/qemu-test.sh` and the selftest proof-manifest.

## Context

Every bind in the system already goes through netstackd's facade, which since TASK-0043 P2 carries the kernel-attributed sender and asks policyd `OP_ABI_EVAL` with the RFC-0091 `net.bind` address class (`loopback` = 127/8 or the facade's loopback emulation; `any` otherwise). What is missing is the policy that makes inbound default-deny meaningful: today any subject with a `net.bind` rule may declare `address = "any"` for itself, nothing fronts an exposed port, and there is no place to attach CIDR allow-lists, rate limits or (later) TLS. `ingressd` does not exist. The identity, deny taxonomy (`ingress-denied`) and counter (`ingress_denies_total{subject}`) already exist (TASK-0043 P2/P4).

## Goals

- Inbound is default-deny: no service binds a non-loopback address; only ONE gateway (`ingressd`) does, and only for exposures declared in the policy SSOT.
- One declarative exposure model (`[[expose."<subject>"]]` in `policies/*.toml`, compiled by the shared grammar) shared by policyd (authority), ingressd (data plane) and `nx` (validation/explain).
- Accept-side enforcement per exposure: CIDR allow-list, deterministic token-bucket rate limit, loopback-only backends.
- Every refusal names a reason from the one deny taxonomy and is counted; every proof is a real behaviour marker (no `ok` for a stub).

## Non-Goals

- TLS/mTLS termination in this RFC's phases (the slot is fixed, the implementation follows the network track).
- Raw-NIC subjects (`device.mmio.net` holders bypass the facade by construction — the grant is the boundary, `docs/security/sandboxing.md`).
- Outbound policy, QUIC/DSoftBus session authorization, IPv6.

## Constraints / invariants (hard requirements)

- Identity is `sender_service_id` from kernel IPC, never a payload field: an exposure intent is attributed to its sender; a forged intent for another subject is rejected (`test_reject_forged_intent_sender`).
- Deny by default at both layers: a bind to `any` without a matching profile rule is refused at the facade; an intent without a declared exposure is refused by ingressd; an accepted peer outside the CIDR allow-list is closed; a peer over the rate is refused.
- Bounded everything: ≤ `MAX_EXPOSURES_PER_SUBJECT` = 8 exposures per subject, ≤ 8 CIDRs per exposure, ≤ 64 open exposures per boot, token bucket (`rate_per_s` ≤ 10 000, `burst` ≤ 1 000), ≤ 16 concurrent forwarded connections per exposure; every bound is a parse error / `STATUS_DENY`, never truncation.
- No kernel change; netstackd stays the only NIC-facing stack in userspace; ingressd forwards to loopback backends over the same facade.
- Markers are behaviour-coupled: `ingressd: port open` only after the listener is bound and the intent registered; `SELFTEST: ingress allow ok` only after bytes crossed the gateway end to end.

## Proposed design

### Contract / interface (normative)

#### 1. Exposure schema (`policies/*.toml`, shared grammar `userspace/policy/src/schema.rs`)

```toml
[[expose."demo.web"]]
port       = 8080                    # 1..=65535, unique per (port, proto) across all subjects
proto      = "tcp"                   # "tcp" | "udp"
cidr_allow = ["10.0.2.0/24"]         # 1..=8 IPv4 CIDRs; "0.0.0.0/0" must be written explicitly
rate_per_s = 100                     # accepted connections/datagrams per second (1..=10000)
burst      = 20                      # token-bucket burst (1..=1000)
tls        = "none"                  # "none" | "tls" | "mtls" — contract slot; only "none" compiles until the network track delivers termination
backend    = 18080                   # loopback backend port the subject listens on (1..=65535)
```

- Attribution: the subject named by the table key must be the kernel-attributed sender of the runtime intent (§3). Unknown keys/sections are parse errors (fail closed). Two exposures with the same (port, proto) are a parse error (`ExposeDuplicate`). `tls != "none"` is a parse error until the slot is delivered (`ExposeTlsUnsupported`) — never a silent downgrade.
- Compiled into policyd's build-time table (next to `ABI_PROFILE_ENTRIES`) and into ingressd's build-time table (same grammar by path); `nx policy validate` checks both consistency rules.

#### 2. Layer A — the bind gate (facade, RFC-0091 `net.bind`)

- The only subject whose RFC-0091 profile may carry `[[net.bind]] address = "any"` is `ingressd`; the shared grammar rejects `address = "any"` for every other subject (`SchemaError::AnyBindNeedsGateway`). Every other service binds loopback-only, so a non-loopback bind without an exposure intent is refused at the facade with `STATUS_DENY`, `!cap-deny: enforcer=netstackd class=net.bind port=<p> addr=any subject=0x<sid>`, policyd audit `reason=ingress-denied`, counter `ingress_denies_total{subject}`.
- Proof: `test_reject_nonloopback_bind_without_intent` (host: the shipped policy compiled + matcher: a non-gateway subject's `any` bind ⇒ deny; the schema rejects the rule) and `SELFTEST: ingress deny ok` (the selftest binds `0.0.0.0:<non-loopback port>` through the facade and receives `STATUS_DENY`).

#### 3. Layer B — `ingressd` (new OS service `source/services/ingressd/`, wire `I`,`G` v1)

| op | request | reply | rule |
|---|---|---|---|
| `OP_EXPOSE = 1` | `{nonce:u32le, port:u16le, proto:u8}` | `{nonce, status, reason:u8}` | sender must be the declared subject of an exposure with that (port, proto); policyd `OP_CHECK_CAP_DELEGATED(sender, "net.expose")` must allow; the gateway binds `any` on `port` through the facade and starts forwarding to `127.0.0.1:backend` (the facade's loopback emulation); ⇒ `ingressd: port open (port=<p>, proto=<tcp\|udp>)` |
| `OP_UNEXPOSE = 2` | `{nonce, port, proto}` | `{nonce, status}` | only the exposing subject (kernel sid) may close its exposure |
| `OP_EXPOSE_STATUS = 3` | `{nonce, port, proto}` | `{nonce, status, open:u8, accepted:u32le, denied_cidr:u32le, denied_rate:u32le}` | authority (`net.expose` holders) observability |

- Statuses: `STATUS_ALLOW = 0`, `STATUS_DENY = 1`, `STATUS_MALFORMED = 2`, `STATUS_UNSUPPORTED = 3`; `reason` on deny: `1 = policy` (no declared exposure / capability), `2 = identity` (sender ≠ subject), `3 = limit` (bounds), `4 = tls` (slot not delivered).
- Accept side (per accepted connection / datagram): peer address must lie in one `cidr_allow` (else close + `ingressd: deny (reason=cidr)`), token bucket per exposure (`rate_per_s`, `burst`, deterministic, refilled from the monotonic clock; over ⇒ close + `ingressd: deny (reason=rate)`); policy refusals print `ingressd: deny (reason=policy)`. Counter `ingress_denies_total{subject}` (shared shape with TASK-0043 P4; ingressd's denies are counted where they happen).
- Forwarding: bounded bidirectional copy between the accepted stream and the backend stream (≤ 16 concurrent per exposure, 4 KiB windows, bounded per-turn budgets so the gateway loop stays reactive); UDP: datagram relay with the source port remembered for the reply path (bounded table).
- Capabilities: `ingressd` holds `net.bind` (`any`), `policy.delegate` (it names the subject it checks), `ipc.core`; exposing services hold `net.expose`. Deny by default in `policies/base.toml`.
- IDL (host SSOT, `tools/nexus-idl/schemas/ingress.capnp`): `ExposeIntent {service, port, proto, cidrAllow, ratePerS, burst, tls, backend}` + `ExposeResponse` — the same fields as §1, for `nx` tooling and host tests; the OS wire above is the ADR-0051 `nexus-wire` frame form.

#### 4. TLS / mTLS slot (contract only)

- `tls = "tls" | "mtls"` selects termination at the gateway with certificates/keys from `configd`/`keystored`; the accept-side filters (§3) run before the handshake. Until the network track delivers it, the grammar rejects the value and the gateway never claims it — no marker, no `ok`.

### Phases / milestones (contract-level)

- **Phase 0** (TASK-0052 P0): this contract, ADR-0061, the IDL seed, `docs/security/ingress.md`.
- **Phase 1** (P1): schema rule `AnyBindNeedsGateway`, corpus fixtures, `test_reject_nonloopback_bind_without_intent`, selftest bind probe, `SELFTEST: ingress deny ok` in headless/smp1.
- **Phase 2** (P2): `[[expose]]` grammar + tables, `source/services/ingressd/` (src/ + tests/), `tests/ingress_host/`: allow, `test_reject_intent_policy_denied`, `test_reject_cidr`, `test_reject_rate_exceeded`, `test_reject_forged_intent_sender`.
- **Phase 3** (P3): init wiring (fixed policyd + netstackd slots — a seam never routes from its hot loop), policy grants, markers `ingressd: ready`, `ingressd: port open (…)`, `ingressd: deny (reason=…)`, `SELFTEST: ingress allow ok`, `SELFTEST: ingress deny ok`, `SELFTEST: ingress rate ok` (single-VM: the selftest exposes a loopback backend, connects through the gateway over QEMU user-net's own address, and drives the rate limit deterministically).

## Security considerations

- Threat model: a service opening a NIC-facing port without review (closed: only the gateway binds `any`, exposures are declared in the policy SSOT, reviewed like capabilities); intent forgery (closed: kernel identity); flood (closed: token bucket + bounded concurrency; refusals are counted, never logged per packet); confused deputy at the gateway (closed: the gateway checks policyd for the *declared subject*, holding `policy.delegate`, and forwards only to loopback).
- Open risks: raw-NIC subjects bypass the facade (documented boundary); TLS slot open (an exposure that needs confidentiality must not be declared `tls = "none"` in production policy — review rule, enforced by `nx policy validate --strict` once the slot lands).

## Failure model (normative)

- Malformed intent / unknown op ⇒ `STATUS_MALFORMED`; undeclared exposure or capability refused ⇒ `STATUS_DENY reason=policy`; sender ≠ subject ⇒ `STATUS_DENY reason=identity`; bounds ⇒ `STATUS_DENY reason=limit`; TLS requested ⇒ `STATUS_DENY reason=tls`.
- policyd unreachable ⇒ intent refused (fail closed); facade unreachable ⇒ `ingressd: port open FAIL` and the intent is not registered; backend down ⇒ accepted connection closed, counted, exposure stays open.
- Every refusal has a stable label: `policy | identity | limit | tls | cidr | rate`.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p policy --test schema_corpus      # AnyBindNeedsGateway, expose fixtures
cd /home/jenning/open-nexus-OS && cargo test -p security_v2_host                 # test_reject_nonloopback_bind_without_intent
cd /home/jenning/open-nexus-OS && cargo test -p ingress_host                     # P2: allow, test_reject_intent_policy_denied, _cidr, _rate_exceeded, _forged_intent_sender
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && just test-os headless   # and smp1
```

### Deterministic markers

- `SELFTEST: ingress deny ok` — a non-loopback bind by a non-gateway subject was refused at the facade (P1).
- `ingressd: ready`, `ingressd: port open (port=<p>, proto=<tcp|udp>)`, `ingressd: deny (reason=policy|cidr|rate)` (P3).
- `SELFTEST: ingress allow ok` — bytes crossed the gateway from a NIC-facing address to a loopback backend; `SELFTEST: ingress rate ok` — the token bucket refused the (burst+1)th attempt within the window (P3).

## Alternatives considered

- Per-service `any` binds guarded only by policy — rejected: no place for CIDR/rate/TLS, N enforcement points instead of one, and every exposed service becomes NIC-facing code.
- A kernel-level firewall — rejected: policy authority stays in userspace (policyd), the kernel has no network stack.
- Putting the gateway into netstackd — rejected: netstackd is the stack, not the policy actor; a separate service keeps the facade's hot loop free of accept-side policy and lets the gateway restart independently (ADR-0057).

## Open questions

- TLS/mTLS termination (owner: network track; needs a `no_std` TLS that fits the OS graph, RFC-0009 dependency hygiene).
- IPv6 CIDRs (reserved with RFC-0091).
- Whether DSoftBus session ports become exposures through this gateway or keep their own authorization (TASK-0030) — decided when the network family resumes.

## Implementation Checklist

- [x] **Phase 0**: contract seed (this RFC), ADR-0061, IDL seed, `docs/security/ingress.md`, RFC/ADR indexes — proof: `just check` docs gates (2026-09-08)
- [x] **Phase 1**: Layer A bind gate — proof: `test_reject_nonloopback_bind_without_intent`, `SELFTEST: ingress deny ok` (2026-09-08)
- [ ] **Phase 2**: `ingressd` host + `tests/ingress_host/` — proof: the five `test_reject_*`
- [ ] **Phase 3**: `ingressd` OS — proof: markers above in headless/smp1
- [ ] Task(s) linked with stop conditions + proof commands (TASK-0052).
- [ ] QEMU markers appear in `scripts/qemu-test.sh` + proof-manifest and pass.
- [ ] Security-relevant negative tests exist (`test_reject_*` above).
