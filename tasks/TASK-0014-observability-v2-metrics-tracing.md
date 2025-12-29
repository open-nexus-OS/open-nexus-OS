---
title: TASK-0014 Observability v2 (OS): metricsd (counters/gauges/histograms) + spans exported via logd
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Depends-on (log sink): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Depends-on (timed metrics): tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

After logd v1 exists (bounded journal + query), the next observability layer is:

- **metrics**: counters/gauges/histograms for low-overhead system health,
- **tracing**: span start/end events with parent/child relationships and attributes.

For v2 we keep it userland-only and export via **structured logs to logd** (no kernel changes).

## Scope note: metrics gatekeeping & retention (v2 requirements)

Metrics must remain stable under real workloads. `metricsd` v2 must therefore include:

- **Cardinality limits**: per-metric and global series caps with deterministic eviction/quarantine behavior.
- **Rate/budget guards**: per-subject EPS/BPS token buckets; excess is dropped with counters and a marker.
- **Downsampling**: raw → 10s → 60s rollups (avg/min/max/sum/count) with bounded, idempotent segment boundaries.
- **TTL/retention**: GC/rotation for WAL/segments with configurable windows and deterministic behavior.

Configuration source of truth (new file):

- `recipes/observability/metrics.toml`

## Goal

In QEMU, prove:

- `metricsd` runs, accepts metric updates, and exports periodic snapshots + span end events to logd.
- A small client library exists (`nexus-metrics`) with macros, usable in OS services/apps.
- Selftest produces deterministic markers proving counters/histograms/spans work.

## Non-Goals

- Full OpenTelemetry compliance.
- High-cardinality label support or unbounded series.
- Remote export network pipeline (future; could layer on dsoftbus).
- Cross-node tracing correlation/propagation (tracked separately as `TASK-0038` to keep this task scoped).

## Constraints / invariants (hard requirements)

- **Kernel untouched**.
- **Bounded memory**:
  - cap number of series, cap live spans, cap per-record payload size.
  - deterministic drop behavior (counter increments continue; series/spans may be dropped with a tracked reason).
- **Determinism**:
  - selftest markers stable;
  - span/trace IDs must be deterministic in OS builds (no RNG dependency) unless we explicitly provide an entropy source.
- **No fake success**: markers only after real updates/exports occurred.
- **Rust hygiene**: no new `unwrap/expect` in OS daemons; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (blocking / must decide now)**:
  - This task depends on **logd v1 (TASK-0006)**. Without logd, exporting “structured logs” is not meaningful;
    we must either postpone v2 or explicitly fall back to UART markers (and label it as a temporary sink).
- **RED (gating)**:
  - Gatekeeping/retention and any on-disk segments require `/state` persistence. Until `TASK-0009` is complete,
    v2 must either:
    - run in RAM-only mode and label it explicitly as non-persistent, or
    - skip retention/segment proofs and only prove bounded in-memory behavior.
- **YELLOW (risky / likely drift / needs follow-up)**:
  - **On-wire contract**: OS-lite currently prefers compact versioned byte frames. Using Cap’n Proto as the only on-wire contract would drift.
    We should use byte frames for OS RPCs and optionally add Cap’n Proto schemas as documentation/future direction.
  - **Span ID model**: random trace/span IDs require entropy. Best-for-OS v2 is a deterministic ID:
    `span_id = (sender_service_id, per-process monotonic counter)` and `trace_id` derived similarly.
  - **Time source**: duration calculations must be robust if the clock is coarse; avoid flakiness in tests by asserting structural properties, not exact timings.
- **GREEN (confirmed assumptions)**:
  - We already have `nexus-log` as the unified facade and will have logd query/stats to validate exports.

## Contract sources (single source of truth)

- `scripts/qemu-test.sh` marker contract.
- log export contract: logd record framing as defined in TASK-0006 (structured record + bounded fields).

## Stop conditions (Definition of Done)

### Proof (Host)

- Add deterministic host tests for registry + histogram bucketing + span lifecycle:
  - `cargo test -p metricsd -- --nocapture` (or a dedicated host test crate if needed)

### Proof (OS / QEMU)

- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - Extend expected markers with:
    - `metricsd: ready`
    - `SELFTEST: metrics counters ok`
    - `SELFTEST: metrics histograms ok`
    - `SELFTEST: tracing spans ok`

Notes:

- Postflight scripts (if added) must **only** delegate to canonical harness/tests; no `uart.log` greps as “truth”.
- Primary proof must validate the **logd export path** (not only a direct scrape).

## Touched paths (allowlist)

- `source/services/metricsd/` (new service)
- `userspace/nexus-metrics/` (new client lib + macros)
- `source/apps/selftest-client/` (markers + deterministic workload)
- `source/services/execd/`, `source/services/bundlemgrd/`, `source/services/dsoftbusd/`, `source/services/timed/` (minimal instrumentation)
- `recipes/observability/metrics.toml` (limits, downsampling, retention defaults)
- `tools/nexus-idl/schemas/` (optional: `metrics.capnp` as schema docs)
- `scripts/qemu-test.sh`
- `docs/observability/`

## Plan (small PRs)

1. **Define OS RPC frames (v1 for metrics/tracing)**
   - Compact, versioned byte frames for:
     - register/lookup series
     - inc/set/observe
     - span start/end
     - optional scrape
   - Cap’n Proto schemas may be added as documentation, but byte frames are authoritative for OS bring-up.

2. **Implement `metricsd`**
   - Bounded in-memory registry (dedupe by name+labels).
   - Counter(u64), Gauge(i64), Histogram(fixed buckets).
   - Gatekeeping v2:
     - per-metric series cap + global cap (evict/quarantine deterministically)
     - per-subject EPS/BPS token buckets with drop counters
   - Optional persistence (gated on TASK-0009):
     - WAL + segment rotation
     - raw→10s→60s rollups
     - TTL GC for segments
   - Span table for live spans; on end emit a structured record (duration/status/attrs).
   - Periodic snapshot export to logd (structured records).
   - Marker: `metricsd: ready`.

3. **Implement `nexus-metrics` client**
   - Host backend for tests; OS backend over kernel IPC.
   - Macros for counters and spans (span guard ends on drop).
   - Deterministic span IDs (no RNG dependency).

4. **Wire minimal instrumentation**
   - `execd`: counters for spawn/deny/fail; span around exec path.
   - `bundlemgrd`: counters and a size histogram (as supported by current OS-lite bundle flows).
   - `dsoftbusd`: session ok/fail + handshake duration histogram (once OS backend exists).
   - `timed`: coalescing delta histogram (TASK-0013).
   - Preserve existing UART readiness markers unchanged.

5. **Selftest**
   - Generate a deterministic workload (fixed number of ops).
   - Validate the export path by querying **logd** for exported records:
     - verify at least one snapshot record exists for the expected series,
     - verify at least one span-end record exists with expected name/status,
     - keep the check bounded (limit + since window).
   - `Scrape` is optional and may exist as a debug/pull path, but it is **not** the primary proof signal.
   - Emit markers:
     - `SELFTEST: metrics counters ok`
     - `SELFTEST: metrics histograms ok`
     - `SELFTEST: tracing spans ok`

6. **Docs**
   - `docs/observability/metrics.md`: naming/labels, histogram buckets, limits.
   - `docs/observability/tracing.md`: span model, deterministic IDs, correlation with logs.

## Acceptance criteria (behavioral)

- Host tests validate registry dedupe, histogram bucketing, span lifecycle deterministically.
- QEMU run prints the new markers and logd shows exported snapshot/span records.
- Kernel unchanged.

## RFC seeds (for later, once green)

- Decisions made:
  - on-wire frames vs schema usage
  - deterministic span/trace ID scheme
  - export cadence and bounds
