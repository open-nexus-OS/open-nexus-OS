---
title: TASK-0038 Tracing v2: cross-node correlation via DSoftBus (context propagation + sampling + traced collector)
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Depends-on (local spans/metrics baseline): tasks/TASK-0014-observability-v2-metrics-tracing.md
  - Depends-on (log sink): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Depends-on (persistence for JSONL): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Depends-on (DSoftBus mux v2): tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md
  - Depends-on (OS DSoftBus networking): tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - Time sync service (placeholder today): source/services/time-syncd/
  - Testing contract: scripts/qemu-test.sh
---

## Context

TASK-0014 covers **local** spans/metrics and exporting span end events via logd. This task extends tracing
to a distributed system:

- stable TraceId/SpanId and trace flags,
- context propagation across process boundaries and across nodes (DSoftBus streams),
- adaptive sampling and privacy rules,
- a collector/ingester (`traced`) that correlates and aligns time across nodes.

Repo reality today:

- OS DSoftBus backend is still a placeholder until networking tasks land.
- Mux v2 is planned (TASK-0020), not implemented yet.
- `/state` persistence is planned (TASK-0009).

Therefore this task must be **host-first** and **OS-gated**, with honest markers only when real behavior exists.

## Goal

Deliver cross-node tracing v2 such that:

- services can create spans with stable ids,
- trace context can be propagated over DSoftBus streams (mux headers),
- traced correlates events by TraceId and writes JSONL (and optionally exports OTLP/HTTP, disabled by default),
- host tests prove propagation + correlation deterministically.

## Non-Goals

- Full OpenTelemetry compliance.
- Tail-based sampling in v2 (possible later).
- Kernel changes.

## Constraints / invariants (hard requirements)

- Kernel untouched.
- Deterministic id generation in OS builds (no RNG dependency unless we provide an explicit entropy source).
- Bounded memory:
  - cap baggage entries,
  - cap live span table,
  - cap “active traces” in traced (LRU).
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake success: “cross-node ok” only after a real remote edge was observed and correlated.

## Red flags / decision points

- **RED (gating)**:
  - Cross-node propagation depends on a real DSoftBus OS backend + mux v2 (TASK-0020 / TASK-0003).
- **YELLOW (clock alignment)**:
  - `time-syncd` is currently a placeholder. v2 must support “best effort” alignment:
    - host tests use an injected clock/offset,
    - OS uses a simple offset provider once time-syncd becomes real.
- **YELLOW (privacy)**:
  - baggage must have a “private:*” convention and must not cross trust boundaries by default.

## Contract sources (single source of truth)

- DSoftBus stream contract: `userspace/dsoftbus` (and mux v2 once implemented)
- log sink semantics: TASK-0006
- marker contract: `scripts/qemu-test.sh` (OS-gated)

## Stop conditions (Definition of Done)

### Proof (Host) — required

New deterministic host tests (`tests/tracing_v2_host/`):

- inject/extract context (compact binary + text-map)
- propagate parent→child across a mock IPC boundary
- cross-node: two in-proc DSoftBus mux sessions exchange a request; traced correlates by TraceId and writes JSONL
- sampling: parent-based sampling is honored; forced-sample list works
- privacy: “private:*” baggage keys are dropped on cross-node injection.

### Proof (OS / QEMU) — gated

Once DSoftBus OS backend + mux v2 exist and `/state` exists:

- `traced: ready`
- `dsoftbus: mux trace caps on`
- `dsoftbus: mux trace propagated`
- `traced: cross-node ok`
- `SELFTEST: mux trace ok`
- `SELFTEST: trace jsonl ok`

## Touched paths (allowlist)

- `userspace/telemetry/` (new trace lib)
- `source/services/traced/` (new collector)
- `userspace/dsoftbus/` (mux trace meta once mux v2 exists)
- `source/apps/selftest-client/` (OS-gated)
- `tests/` (host tests)
- `docs/observability/tracing.md`
- `scripts/qemu-test.sh` (OS-gated)

## Plan (small PRs)

1. **Core trace context library (`nexus-trace`)**
   - TraceId (128-bit), SpanId (64-bit), flags, bounded baggage.
   - Deterministic id generation scheme suitable for OS builds.
   - Inject/extract into:
     - compact binary header (for mux),
     - text-map (for debugging/tools).

2. **Propagation**
   - DSoftBus mux: attach trace meta to stream open (once mux v2 exists).
   - Optional IPC propagation: provide helpers for services to attach trace meta to their byte frames
     (do not require kernel changes or universal adoption in v2).

3. **Collector (`traced`)**
   - Ingest spans/events via:
     - direct IPC from services (preferred),
     - and/or logd records (fallback) once logd exists.
   - Correlate by TraceId; write JSONL to `/state/trace/*.jsonl` (gated on statefs).
   - Optional OTLP/HTTP exporter disabled by default (feature flag + runtime switch).

4. **Sampling policy**
   - ParentBased sampler with default rate (e.g., 10% roots).
   - Force-sample allowlist by span name.
   - Drop private baggage keys on cross-node propagation.

5. **Docs**
   - Trace model, propagation rules, sampling and privacy.

