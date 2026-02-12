---
title: TASK-0014 Observability v2 (OS): metricsd (counters/gauges/histograms) + spans exported via logd
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Seed contract: docs/rfcs/RFC-0024-observability-v2-metrics-tracing-contract-v1.md
  - Depends-on (log sink): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Depends-on (persistence substrate): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Depends-on (timed metrics): tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md
  - IPC correlation contract: docs/rfcs/RFC-0019-ipc-request-reply-correlation-v1.md
  - Testing contract: scripts/qemu-test.sh
enables:
  - TASK-0038: Tracing v2 cross-node correlation (context propagation + sampling + traced collector)
  - TASK-0040: Remote observability v1 over DSoftBus (scrape/query pipeline over network)
  - TASK-0041: Lock profiling v1 userspace export hooks (optional sinks into metrics/tracing)
  - TASK-0143: Perf v1a perfd frame trace + bounded metrics export
  - TASK-0046: Config v1 schemas/layering consume metrics/tracing policy surfaces
follow-up-tasks:
  - TASK-0038: extend local span model to cross-node propagation/correlation only after local v2 contract is proven
  - TASK-0040: add remote scrape/collector path over DSoftBus on top of local logd/metricsd exports
  - TASK-0041: consume metrics/tracing sinks as optional lock-profiling export target
  - TASK-0143: build UI/perf-focused tracing and metrics on top of shared observability primitives
  - TASK-0046: align config schemas and rollout controls for metrics/tracing retention and budgets
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

- `logd` sink hardening baseline is enforced before `metricsd` high-volume export paths are enabled.
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
- **Ownership/type/concurrency floor**:
  - use explicit newtypes at contract boundaries (for example `MetricKey`, `SeriesId`, `SpanId`, `TraceId`, bounded field wrappers) instead of raw integer/string plumbing.
  - keep ownership explicit across decode -> registry -> export; avoid hidden shared mutable state.
  - preserve explicit `Send`/`Sync` boundaries; no new `unsafe impl Send/Sync` without written safety argument and tests.
  - define newtypes and boundary assertions early even where a first slice still runs single-threaded.
- **Determinism**:
  - selftest markers stable;
  - span/trace IDs must be deterministic in OS builds (no RNG dependency) unless we explicitly provide an entropy source.
  - QEMU proof runs use modern virtio-mmio defaults for deterministic reproducibility, including logd-hardening-only slices (legacy mode only for debug/bisect).
- **No fake success**: markers only after real updates/exports occurred.
- **Rust hygiene**: no new `unwrap/expect` in OS daemons; no blanket `allow(dead_code)`.

## Explicit prerequisites (from TASK-0006 / TASK-0009 / RFC-0011)

This task assumes:

- `logd` v1 exists and supports bounded `APPEND/QUERY/STATS` (byte frames).
- `/state` substrate from `TASK-0009` exists for bounded WAL/segment persistence.
- OS services can emit structured records via `nexus-log` → logd (best-effort; UART fallback allowed).
- The log record includes authenticated origin (`sender_service_id`) and a bounded opaque `fields` blob.
- The `fields` blob follows the RFC-0011 deterministic convention (`key=value\n`, sorted by key) for interoperability.
- `timed` coalescing baseline from `TASK-0013` exists and can be instrumented as a producer.

This task does **not** assume:

- That “all services are already migrated” to `nexus-log`. Instrumentation is part of this task and must keep existing UART markers unchanged.
- Any logd persistence, streaming subscriptions, or remote export; those remain out of scope until their dedicated tasks land.

## Red flags / decision points

- **RED (scope guard / must enforce now)**:
  - Scope boundary must stay strict: local metrics/spans + logd export only.
    Remote scrape/correlation work belongs to `TASK-0040`/`TASK-0038`; do not backdoor it here.
- **YELLOW (risky / likely drift / needs follow-up)**:
  - **On-wire contract**: OS-lite currently prefers compact versioned byte frames. Using Cap’n Proto as the only on-wire contract would drift.
    We should use byte frames for OS RPCs and optionally add Cap’n Proto schemas as documentation/future direction.
  - **Span ID model**: random trace/span IDs require entropy. Best-for-OS v2 is a deterministic ID:
    `span_id = (sender_service_id, per-process monotonic counter)` and `trace_id` derived similarly.
  - **Time source**: duration calculations must be robust if the clock is coarse; avoid flakiness in tests by asserting structural properties, not exact timings.
- **GREEN (confirmed assumptions)**:
  - `TASK-0006` is done: `logd` exists as bounded export sink.
  - `TASK-0009` is done: `/state` substrate exists for retention/WAL slices in this task.
  - We already have `nexus-log` as the unified facade and can use logd query/stats to validate exports.

## Security considerations

`TASK-0014` is security-relevant: it adds new IPC surfaces, accepts untrusted telemetry payloads,
and exports structured data that may accidentally carry sensitive values.

### Threat model

- Malicious or buggy services flood `metricsd` with high-cardinality labels to trigger memory pressure.
- Producers send oversized metric/span payloads to force parse/alloc failures.
- Producers attempt identity spoofing via payload fields instead of authenticated sender metadata.
- Sensitive values (tokens, keys, credentials, PII) leak through labels/attributes into `logd`.

### Security invariants (MUST hold)

- All producer identity/policy decisions use authenticated kernel metadata (`sender_service_id`), never payload claims.
- Every untrusted input is bounded before parse/alloc: series count, span count, labels count, key/value length, frame size.
- Rejection and throttling are deterministic and auditable (`invalid_args`, `over_limit`, `rate_limited`) with bounded counters.
- No secret-bearing fields are exported to `logd`; sensitive keys are denied or redacted by default policy.
- Metrics/tracing must not create a second policy authority; privileged decisions remain in `policyd`.

### DON'T DO (explicit prohibitions)

- Don't trust payload-provided service names or IDs for authorization/routing decisions.
- Don't accept unbounded series cardinality, span tables, or attribute payload sizes.
- Don't log secrets/plaintext credentials in markers, logs, attributes, or error messages.
- Don't emit `metricsd: ready` or `SELFTEST: ... ok` markers before real export behavior is proven.
- Don't add random/non-deterministic proof markers that break reproducibility.

### Attack surface impact

- Moderate increase: new local IPC entry points and structured export path.
- Bounded by strict limits, deterministic rejection, and authenticated service identity checks.

### Mitigations

- Enforce per-metric and global series caps, live-span caps, per-subject EPS/BPS budgets.
- Validate and bound all fields at decode boundary; reject malformed frames with explicit status.
- Maintain allowlist/denylist policy for attribute keys to prevent secret leakage.
- Add negative tests for malformed/oversized/rate-abuse requests and preserve deterministic markers.

## Security proof (security-relevant)

### Audit tests (negative cases / attack simulation)

- Command(s):
  - `cargo test -p logd -- reject --nocapture`
  - `cargo test -p metricsd -- reject --nocapture`
- Required tests:
  - `test_reject_log_append_oversized_fields`
  - `test_reject_log_append_rate_limited_sender`
  - `test_reject_log_payload_identity_spoof`
  - `test_reject_oversized_metric_fields`
  - `test_reject_series_cap_exceeded`
  - `test_reject_live_span_cap_exceeded`
  - `test_reject_payload_identity_spoof`
  - `test_reject_rate_limit_exceeded`

### Hardening markers (QEMU)

- `logd: reject invalid_args`
- `logd: reject over_limit`
- `logd: reject rate_limited`
- `SELFTEST: logd hardening rejects ok`
- `metricsd: reject invalid_args`
- `metricsd: reject over_limit`
- `metricsd: reject rate_limited`
- `SELFTEST: metrics security rejects ok`

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
- Proofs should include at least one host assertion for newtype decode/validation boundaries and one assertion for `Send`/`Sync` boundary expectations.

### Soll-first deterministic test matrix

- **Feature: Phase 0a logd hardening (sink preflight)**
  - Host: reject tests for oversized fields, payload identity spoof, and per-sender rate-limit.
  - QEMU: `logd: reject invalid_args|over_limit|rate_limited` + `SELFTEST: logd hardening rejects ok`.
  - No-fake check: invalid-only traffic must never emit metrics success markers.
- **Feature: metrics semantics (counter/gauge/histogram)**
  - Host: deterministic tests for counter monotonicity, gauge set semantics, histogram bucket boundaries with fixed vectors.
  - QEMU: `SELFTEST: metrics counters ok` and `SELFTEST: metrics histograms ok` only after logd-exported evidence is observed.
- **Feature: tracing span lifecycle**
  - Host: deterministic span start/end pairing and duration/status serialization checks.
  - QEMU: `SELFTEST: tracing spans ok` only after at least one span-end record is queryable in logd.
- **Feature: ownership/newtype/send-sync boundaries**
  - Host: compile-time/runtime boundary tests for newtype decode/reject and thread-safety constraints.
  - Policy: no new `unsafe impl Send/Sync` unless a dedicated safety argument and test are added in the same slice.
- **Feature: determinism floor (modern MMIO)**
  - QEMU proofs run on modern virtio-mmio defaults and bounded timeouts; legacy mode is debug-only.

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

1. **Phase 0a: harden `logd` sink for observability load**
   - Bound APPEND field size/count/key-length/value-length with deterministic rejects.
   - Enforce sender identity from kernel metadata (`sender_service_id`), never payload claim.
   - Add per-sender budget/rate guards and explicit reject counters/markers.
   - Keep this slice small and deterministic; no feature expansion beyond sink hardening.

2. **Define OS RPC frames (v1 for metrics/tracing)**
   - Compact, versioned byte frames for:
     - register/lookup series
     - inc/set/observe
     - span start/end
     - optional scrape
   - Cap’n Proto schemas may be added as documentation, but byte frames are authoritative for OS bring-up.

3. **Implement `metricsd`**
   - Bounded in-memory registry (dedupe by name+labels).
   - Counter(u64), Gauge(i64), Histogram(fixed buckets).
   - Gatekeeping v2:
     - per-metric series cap + global cap (evict/quarantine deterministically)
     - per-subject EPS/BPS token buckets with drop counters
   - Persistence slice (uses `TASK-0009` `/state` substrate):
     - WAL + segment rotation
     - raw→10s→60s rollups
     - TTL GC for segments
   - Span table for live spans; on end emit a structured record (duration/status/attrs).
   - Periodic snapshot export to logd (structured records).
   - Marker: `metricsd: ready`.

4. **Implement `nexus-metrics` client**
   - Host backend for tests; OS backend over kernel IPC.
   - Macros for counters and spans (span guard ends on drop).
   - Deterministic span IDs (no RNG dependency).
   - Introduce newtype wrappers early even where first call sites are still minimal.

5. **Wire minimal instrumentation**
   - `execd`: counters for spawn/deny/fail; span around exec path.
   - `bundlemgrd`: counters and a size histogram (as supported by current OS-lite bundle flows).
   - `dsoftbusd`: session ok/fail + handshake duration histogram (once OS backend exists).
   - `timed`: coalescing delta histogram (TASK-0013).
   - Preserve existing UART readiness markers unchanged.

6. **Selftest**
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

7. **Docs**
   - `docs/observability/metrics.md`: naming/labels, histogram buckets, limits.
   - `docs/observability/tracing.md`: span model, deterministic IDs, correlation with logs.

## Acceptance criteria (behavioral)

- Log sink hardening rejects malformed/oversized/rate-abusive append traffic deterministically before metrics load is enabled.
- Host tests validate registry dedupe, histogram bucketing, span lifecycle deterministically.
- QEMU run prints the new markers and logd shows exported snapshot/span records.
- Kernel unchanged.

## RFC seeds (for later, once green)

- Decisions made:
  - on-wire frames vs schema usage
  - deterministic span/trace ID scheme
  - export cadence and bounds
