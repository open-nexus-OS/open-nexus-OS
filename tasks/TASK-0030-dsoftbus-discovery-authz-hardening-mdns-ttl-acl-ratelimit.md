---
title: TASK-0030 DSoftBus discovery/authz hardening: NXSB TTL/backoff + pre-session ACL + rate-limits (rebased 2026-08-14 onto shipped NXSB discovery; host-first, OS-gated)
status: Draft
owner: @runtime
created: 2025-12-22
depends-on:
  - TASK-0003
  - TASK-0020
  - TASK-0021
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - ADR: docs/adr/0005-dsoftbus-architecture.md
  - DSoftBus overview: docs/distributed/dsoftbus-lite.md
  - Depends-on (OS dsoftbus networking): tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - Depends-on (mux v2): tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md
  - Depends-on (transport kinds): tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md
  - Testing contract: scripts/qemu-test.sh
---

## Short description

- **Scope**: Harden the shipped NXSB discovery and admission path with TTL/backoff, pre-session ACL
  checks, and rate limits.
- **Deliver**: Host-first negative/abuse-case proofs and fail-closed pre-session policy checks on
  top of the existing bounded peer cache.
- **Out of scope**: Transport redesign, kernel/network stack expansion, and any mDNS/DNS-SD stack.

## Rebase note (2026-08-14) — verified against repo reality

This ledger was drafted (2025-12-22) around an mDNS SRV/TXT discovery design **the repo never
built**. Repo reality:

- Discovery is a custom bounded binary packet, magic `NXSB`, versioned, length-prefixed
  (`source/libs/nexus-discovery-packet/src/lib.rs:41` `MAGIC = *b"NXSB"`, `:42` `VERSION_V1`;
  a `no_std` port of `userspace/dsoftbus/src/discovery_packet.rs`, see header at `:15`), carried
  over UDP announce/listen in `source/services/dsoftbusd/src/os/discovery/`.
- A bounded LRU peer cache **already shipped**: `source/libs/nexus-peer-lru` with
  `MAX_PEERS = 16` (`src/lib.rs:25`) — do NOT re-implement peer-table boundedness or LRU eviction.
- The old claim "DSoftBus OS backend is a placeholder (`todo!()`)" is stale for discovery: only
  `userspace/dsoftbus/src/os.rs` remains an honest placeholder shim; dsoftbusd's own OS path
  bypasses it and does real UDP discovery today.

**Residual scope, rewritten against NXSB (all mDNS SRV/TXT goals/DoD items are struck):**

1. TTL aging + exponential backoff on NXSB announcements and cached peers (the LRU cache has no
   TTL/backoff semantics today).
2. Pre-session ACL: deny-by-default admission check before connect/accept.
3. Rate limits: deterministic token buckets for announcement processing and handshake attempts.

Security invariants and the `test_reject_*` discipline below stay in force unchanged (with mDNS
payload cases retargeted at NXSB packets).

**Hard gate on OS proofs (2026-08-14):** OS/QEMU proofs are blocked until
`tasks/TRACK-NETWORK-PROOF-LANES.md` is repaired — the 2-VM lane currently dies in
`OS2VM_E_DISCOVERY_TIMEOUT` (both nodes discover zero peers), so cross-device discovery markers
cannot be asserted. Host-first work can proceed now.

## Production Closure Phases (RFC-0034 alignment)

This task follows the shared production gate profile (`Core + Performance`) from `RFC-0034`.
No phase may be marked green without the linked proof evidence.

- **Phase A (Contract lock)**: lock ACL authority, TTL/backoff contract, and deterministic rate-limit semantics.
- **Phase B (Host proof)**: requirement-named host abuse/reject suites are green.
- **Phase C (OS-gated proof)**: canonical marker ladder is green once OS backend supports these paths.
- **Phase D (Performance gate)**: bounded discovery/admission behavior under load is proven with deterministic workloads.
- **Phase E (Closure & handoff)**: docs/testing + board/order + RFC state are synchronized with proof evidence, and for distributed claims the `tools/os2vm.sh` release artifacts are reviewed (`summary.{json,txt}` + `release-evidence.json`).

Canonical gate commands:

- Host: task-owned requirement suites for ACL/rate-limit/TTL behavior.
- OS: `cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
- 2-VM distributed: `cd /home/jenning/open-nexus-OS && RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
- Regression: `cd /home/jenning/open-nexus-OS && just test-e2e && just test-os-dhcp`
- Release evidence review (if distributed behavior is asserted): `artifacts/os2vm/runs/<runId>/summary.{json,txt}` and `artifacts/os2vm/runs/<runId>/release-evidence.json`

## Context

DSoftBus discovery and admission need hardening to be robust against:

- flapping peers,
- discovery flooding,
- handshake spamming,
- unauthorized peers attempting to connect before policy checks.

We want TTL + backoff on the shipped NXSB discovery, and a pre-session ACL check.

Repo reality today (see rebase note above):

- NXSB binary discovery packets + bounded LRU peer cache are shipped and in use.
- OS/QEMU cross-device proof is **gated** on the TRACK-NETWORK-PROOF-LANES repair
  (`OS2VM_E_DISCOVERY_TIMEOUT`).

## Goal

Implement discovery/authz hardening without changing transports or mux:

- TTL aging + exponential backoff for NXSB-announced peers (extending `nexus-peer-lru` semantics,
  not replacing the cache).
- pre-session admission control (ACL) before connect/accept.
- rate limits for discovery processing and handshake attempts.
- deterministic host tests; OS markers only once the proof lanes are green.

## Non-Goals

- Replacing Noise handshake.
- Changing TCP/QUIC transport implementations.
- Kernel changes.

## Constraints / invariants (hard requirements)

- Kernel untouched.
- Deterministic behavior:
  - TTL/backoff based on an injectable clock in tests.
  - rate limiting deterministic (token bucket with fixed parameters).
- Bounded memory:
  - peer table stays bounded — already shipped as `nexus-peer-lru` (`MAX_PEERS = 16`, LRU
    eviction); do not re-implement.
  - NXSB packet parsing stays bounded (the packet lib already enforces length-prefixed caps;
    keep them when extending).
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- **Approval-gated zone**: `source/libs/**` (ABI stability) — any change to
  `nexus-discovery-packet` or `nexus-peer-lru` needs explicit user approval before modification.

## Red flags / decision points

- **RED (OS gating)**:
  - OS markers are blocked until `tasks/TRACK-NETWORK-PROOF-LANES.md` is green
    (os2vm lane fails in `OS2VM_E_DISCOVERY_TIMEOUT`).
- **YELLOW (discovery scope)**:
  - Stay on the NXSB packet format; version bumps go through `nexus-discovery-packet`
    (approval-gated). Do not grow an mDNS/DNS-SD stack in v1.
- **YELLOW (ACL authority)**:
  - Keep ACL simple and deterministic, and document its relationship to policyd/nexus-sel.
  - Prefer allow-by-default = false.

## Security considerations

### Threat model

- Discovery flooding and handshake spamming to exhaust resources.
- Unauthorized peers attempting pre-session connection before policy checks.
- Malformed NXSB discovery packets attempting parser or bounds failures.

### Security invariants (MUST hold)

- Admission is deny-by-default until ACL/policy allows.
- Discovery parsing is bounded and deterministic.
- Rate-limit and backoff decisions are enforced before expensive session work.

### DON'T DO (explicit prohibitions)

- DON'T perform connect/accept before ACL/policy checks.
- DON'T parse unbounded discovery payloads.
- DON'T downgrade authz failures into warning-only behavior.

### Attack surface impact

- Significant: network-discovery and admission-control surface.

### Mitigations

- Bounded peer table (shipped `nexus-peer-lru`) + NXSB packet caps, deterministic token buckets,
  and explicit pre-session policy gate.

## Security proof

### Audit tests (negative cases / attack simulation)

- Commands:
  - `cargo test -p dsoftbus -- discovery --nocapture`
- Required tests:
  - `test_reject_peer_outside_acl`
  - `test_reject_discovery_rate_limit_exceeded`
  - `test_reject_oversize_discovery_packet`

### Hardening markers (QEMU, if applicable)

- `dsoftbusd: discovery up (udp)`
- `dsoftbusd: remote proxy denied (unauthenticated)`

## Contract sources (single source of truth)

- Discovery packet format (NXSB): `source/libs/nexus-discovery-packet` (approval-gated zone)
- Peer cache: `source/libs/nexus-peer-lru` (approval-gated zone)
- DSoftBus backend traits: `userspace/dsoftbus`
- DSoftBus discovery docs: `docs/distributed/dsoftbus-lite.md`
- QEMU marker contract: `scripts/qemu-test.sh` (gated)

## Stop conditions (Definition of Done)

### Proof (Host) — required

Add deterministic host tests (`tests/dsoftbus_discovery_host/`):

- TTL aging: peers expire after TTL
- backoff: failed connects increase delay up to a cap; refreshed by a new NXSB announcement
  (proof-of-life)
- ACL deny: denied peers never attempt connect/accept
- rate limiting: NXSB announcement floods do not crash; limiter triggers and drops excess.

### Proof (OS / QEMU) — after proof lanes are green (TRACK-NETWORK-PROOF-LANES)

Extend `scripts/qemu-test.sh` (order tolerant) with:

- `dsoftbusd: peer add`
- `dsoftbusd: acl enforced`
- `SELFTEST: acl allow ok`
- `SELFTEST: acl deny ok`
- `dsoftbusd: backoff`
- `SELFTEST: backoff ok`

Notes:

- Postflight scripts must delegate to canonical tests/harness; no independent “log greps = success”.

## Touched paths (allowlist)

- `userspace/dsoftbus/` (host discovery hardening, rate limiters)
- `source/services/dsoftbusd/` (OS discovery integration + markers)
- `source/libs/nexus-discovery-packet/`, `source/libs/nexus-peer-lru/` (only if the contract must
  grow — approval-gated zone, explicit user approval required)
- `recipes/dsoftbus/acl.toml` (new)
- `tests/` (host tests)
- `docs/distributed/` and `docs/security/`
- `scripts/qemu-test.sh` (gated)

## Plan (small PRs)

1. **Announcement TTL/backoff (host-first, NXSB)**
   - Announcement cadence with token-bucket limit for announce/processing (e.g., 10pps burst 20),
     on the existing NXSB packet format (no wire change expected).

2. **Peer cache TTL + backoff (extend, don't rebuild)**
   - Extend the shipped `nexus-peer-lru` semantics (bounded size + LRU eviction already exist)
     with:
     - TTL aging (default 60s)
     - exponential backoff on connect failures (1s..60s).
   - Markers:
     - `dsoftbusd: peer add <id>@<ip>:<port> tr=<transport>`
     - `dsoftbusd: peer expire <id>`
     - `dsoftbusd: backoff <id> <delay_ms>`.

3. **Pre-session ACL**
   - `recipes/dsoftbus/acl.toml` with allow-by-default=false.
   - Matchers:
     - device id exact / prefix glob
     - optional transport kind constraint
     - allowed service names list.
   - Enforce before connect/accept; log audited denies (to logd if available).
   - Marker: `dsoftbusd: acl enforced`.

4. **Rate limits**
   - handshake attempt limiter per peer (e.g., ≤3 / 30s)
   - discovery processing limiter global (e.g., ≤50 packets / 5s)
   - marker: `dsoftbusd: rate-limit active (handshake|discovery)`.

5. **Docs**
   - `docs/distributed/discovery.md`: NXSB packet schema, TTL/backoff, rate limits.
   - `docs/security/dsoftbus-acl.md`: ACL schema and examples.
