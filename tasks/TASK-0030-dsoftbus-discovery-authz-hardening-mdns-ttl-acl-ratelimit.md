---
title: TASK-0030 DSoftBus discovery/authz hardening: mDNS SRV/TXT + TTL/backoff + pre-session ACL + rate-limits (host-first, OS-gated)
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - ADR: docs/adr/0005-dsoftbus-architecture.md
  - DSoftBus overview: docs/distributed/dsoftbus-lite.md
  - Depends-on (OS dsoftbus networking): tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - Depends-on (mux v2): tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md
  - Depends-on (transport kinds): tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

DSoftBus discovery and admission need hardening to be robust against:

- flapping peers,
- discovery flooding,
- handshake spamming,
- unauthorized peers attempting to connect before policy checks.

We want mDNS SRV/TXT discovery (service metadata), TTL + backoff, and a pre-session ACL check.

Repo reality today:

- DSoftBus host backend is functional.
- DSoftBus OS backend is a placeholder (`todo!()`), so OS/QEMU proof is **gated**.

## Goal

Implement discovery/authz hardening without changing transports or mux:

- mDNS publishing/resolution with SRV/TXT.
- peer cache with TTL and exponential backoff.
- pre-session admission control (ACL) before connect/accept.
- rate limits for discovery processing and handshake attempts.
- deterministic host tests; OS markers only once OS DSoftBus backend exists.

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
  - peer table has a max size; evict LRU.
  - mDNS TXT parsing size caps.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (OS gating)**:
  - OS markers are blocked until TASK-0003 makes OS DSoftBus real.
- **YELLOW (mDNS scope)**:
  - Implement minimal mDNS needed for `_nexus._tcp.local` / `_nexus._udp.local` SRV/TXT only.
  - Do not grow into a general-purpose DNS stack in v1.
- **YELLOW (ACL authority)**:
  - Keep ACL simple and deterministic, and document its relationship to policyd/nexus-sel.
  - Prefer allow-by-default = false.

## Contract sources (single source of truth)

- DSoftBus backend traits: `userspace/dsoftbus`
- DSoftBus discovery docs: `docs/distributed/dsoftbus-lite.md`
- QEMU marker contract: `scripts/qemu-test.sh` (gated)

## Stop conditions (Definition of Done)

### Proof (Host) — required

Add deterministic host tests (`tests/dsoftbus_discovery_host/`):

- SRV/TXT encode/decode + publish/resolve
- TTL aging: peers expire after TTL
- backoff: failed connects increase delay up to a cap; refreshed by new mDNS proof-of-life
- ACL deny: denied peers never attempt connect/accept
- rate limiting: discovery floods do not crash; limiter triggers and drops excess.

### Proof (OS / QEMU) — after OS backend exists

Extend `scripts/qemu-test.sh` (order tolerant) with:

- `dsoftbus: mdns announce ok`
- `dsoftbusd: peer add`
- `dsoftbusd: acl enforced`
- `SELFTEST: acl allow ok`
- `SELFTEST: acl deny ok`
- `dsoftbusd: backoff`
- `SELFTEST: backoff ok`

Notes:

- Postflight scripts must delegate to canonical tests/harness; no independent “log greps = success”.

## Touched paths (allowlist)

- `userspace/dsoftbus/` (mdns module, peer cache, rate limiters)
- `source/services/dsoftbusd/` (integration + markers, once implementation exists)
- `recipes/dsoftbus/acl.toml` (new)
- `tests/` (host tests)
- `docs/distributed/` and `docs/security/`
- `scripts/qemu-test.sh` (gated)

## Plan (small PRs)

1. **mDNS module (host-first)**
   - `_nexus._tcp.local` and `_nexus._udp.local` SRV + TXT records:
     - `ver=1`, `transport=tcp|quic|udp-sec`, `mux=v2`, `services=...`, `device=<id>`
   - Token-bucket limit for publish/query (e.g., 10pps burst 20).
   - Marker on first announce: `dsoftbus: mdns announce ok`.

2. **Peer cache (TTL + backoff)**
   - `PeerTable` with:
     - TTL aging (default 60s)
     - exponential backoff on connect failures (1s..60s)
     - bounded size + LRU eviction.
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
   - discovery processing limiter global (e.g., ≤50 records / 5s)
   - marker: `dsoftbusd: rate-limit active (handshake|mdns)`.

5. **Docs**
   - `docs/distributed/discovery.md`: SRV/TXT schema, TTL/backoff, rate limits.
   - `docs/security/dsoftbus-acl.md`: ACL schema and examples.
