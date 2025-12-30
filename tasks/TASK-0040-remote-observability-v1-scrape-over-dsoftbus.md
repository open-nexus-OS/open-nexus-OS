---
title: TASK-0040 Remote observability v1: scrape logs/metrics over DSoftBus (host-first, OS-gated; VMO backfill deferred)
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - DSoftBus base: docs/distributed/dsoftbus-lite.md
  - Depends-on (logd v1): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Depends-on (metrics v2): tasks/TASK-0014-observability-v2-metrics-tracing.md
  - Depends-on (DSoftBus OS bring-up): tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - Depends-on (mux v2): tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md
  - Depends-on (ACL hardening): tasks/TASK-0030-dsoftbus-discovery-authz-hardening-mdns-ttl-acl-ratelimit.md
  - Depends-on (VMO plumbing; optional): tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We want remote collection of logs and metrics across nodes via DSoftBus with:

- ACL and rate limits,
- sampling for logs,
- retention/rotation on the collector,
- a small query API for recent data.

Repo reality today:

- logd v1 and metricsd are still planned tasks (not implemented).
- OS DSoftBus backend is a placeholder until networking tasks land.
- Mux v2 is planned (TASK-0020).
- True “VMO bulk backfill over DSoftBus” depends on VMO sharing + mux VMO frames (not available yet).

Therefore v1 must be **host-first** and **OS-gated**, and VMO backfill must be explicitly deferred.

## Goal

Deliver remote observability v1 where, on host builds:

- a “server” provides `obs.logs` and `obs.metrics` endpoints over DSoftBus (using existing host backend),
- a “collector” connects to peers, enforces ACL/rate/sampling, stores data with rotation,
- deterministic tests prove correctness (including negative cases).

Once OS prerequisites exist, add QEMU markers and enable the same flow on OS.

## Non-Goals

- VMO-based bulk log backfill (deferred to follow-up once VMO + mux v2 VMO frames exist).
- Full OpenTelemetry compliance.
- Kernel changes.

## Constraints / invariants (hard requirements)

- Kernel untouched.
- Deterministic and bounded:
  - rate limit token buckets with fixed parameters,
  - bounded in-memory buffers,
  - bounded file retention/rotation.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake markers.

## Red flags / decision points

- **RED (gating)**:
  - OS implementation is blocked until:
    - logd v1 exists (TASK-0006),
    - metricsd exists (TASK-0014),
    - DSoftBus OS backend exists (TASK-0003 + TASK-0020).
- **YELLOW (wire formats)**:
  - Logs as JSONL is fine, but must be bounded and optionally sampled.
  - Metrics deltas must cap label cardinality; prefer a small, versioned frame format.
- **YELLOW (backfill)**:
  - Backfill over the network should not be implemented until VMO plumbing is proven; otherwise it will devolve into large copy paths.

## Contract sources (single source of truth)

- DSoftBus stream/session traits: `userspace/dsoftbus`
- log sink contract: TASK-0006
- metrics/span export contract: TASK-0014

## Stop conditions (Definition of Done)

### Proof (Host) — required

New deterministic host tests (`tests/remote_obs_host/`):

- log stream:
  - sampling reduces volume deterministically
  - rate limiting caps events/sec and does not crash
- metrics stream:
  - delta frames reconstruct counters/gauges/histograms
  - label cardinality caps are enforced
- ACL deny:
  - denied peers are not connected
- retention:
  - rotation triggers on size/time budget and is deterministic.

### Proof (OS / QEMU) — gated

Once OS prerequisites exist, extend `scripts/qemu-test.sh` with:

- `logd: obs logs live ready`
- `metricsd: obs metrics live ready`
- `obsscraped: connect logs <peer> ok`
- `obsscraped: connect metrics <peer> ok`
- `SELFTEST: obs logs connect ok`
- `SELFTEST: obs metrics connect ok`
- `SELFTEST: obs logs rate/sampling ok`
- `SELFTEST: obs metrics query ok`

## Touched paths (allowlist)

- `userspace/dsoftbus/` (host integration for obs streams)
- `source/services/logd/` (once implemented; expose obs.logs endpoint)
- `source/services/metricsd/` (once implemented; expose obs.metrics endpoint)
- `source/services/obsscraped/` (new collector service; host-first)
- `tests/`
- `docs/observability/remote.md`
- `scripts/qemu-test.sh` (gated)

## Plan (small PRs)

1. **Define service names + wire formats**
   - DSoftBus services: `obs.logs`, `obs.metrics`.
   - Logs: JSONL lines with a small header for sampling/rate config.
   - Metrics: compact binary delta frames with version byte; bounded labels.

2. **Host-only implementations**
   - Implement an in-proc `obs.logs` and `obs.metrics` server for tests (can live inside the test crate initially).
   - Implement `obsscraped` collector:
     - connects to peers (host discovery),
     - enforces ACL/rate/sampling,
     - writes rotated files under a host temp dir (in tests).

3. **Query API (host-first)**
   - Local-only API (CLI or simple in-proc query) for:
     - “recent metrics snapshot”
     - “recent logs window”.
   - Keep network binding out of scope until sandboxing/ABI policy exists.

4. **Docs**
   - Document formats, limits, ACL expectations, and deferred backfill plan.

## Follow-ups (separate tasks)

- VMO bulk backfill once:
  - VMO sharing is proven (TASK-0031) and
  - mux v2 can carry bulk descriptors safely (TASK-0020 extension).

