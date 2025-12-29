---
title: TASK-0143 Perf v1a (host-first): perfd frame pacing tracer + Chrome Trace export + bounded metrics
status: Draft
owner: @reliability
created: 2025-12-25
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Metrics/tracing foundations: tasks/TASK-0014-observability-v2-metrics-tracing.md
  - Persistence (/state for trace export): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Storage error semantics (for file export errors): tasks/TASK-0132-storage-errors-vfs-semantic-contract.md
---

## Context

We want performance work to be **provable** and **regression-resistant**:

- collect per-frame timings (UI/layout/render/present),
- emit bounded traces exportable in a standard format (Chrome Trace JSON),
- expose live HUD metrics (fps/p95/jank) to SystemUI.

This is **not** a general-purpose tracing stack replacement (that is `TASK-0014`). Instead, `perfd` is a
UI/perf focused tracer designed around deterministic frame pacing instrumentation.

Scope note:

- Perf v2 refresh (sessions/budgets/scenarios/reports) is tracked as `TASK-0172`/`TASK-0173`.
  If v2 lands, treat this v1 task as superseded rather than maintaining parallel perf infrastructures.

## Goal

Deliver:

1. `perfd` service:
   - maintains an in-memory ring of the last N frames (default 1200)
   - records paired marks (begin/end) and per-frame ticks
   - computes metrics:
     - fps, avg, p95, p99, longFrames, dropped, frames, budgetMs
   - exports last session in Chrome Trace JSON (`traceEvents`) form
2. API (Cap’n Proto schema):
   - `tools/nexus-idl/schemas/perf.capnp` defining start/stop/markBegin/markEnd/frameTick/exportLast
3. Trace export:
   - host-first: export to a temp directory (test fixture)
   - OS-gated: export to `state://perf/traces/<name>-<ts>.json` once `/state` exists
4. Markers:
   - `perfd: ready`
   - `perf: start <name> (budget=...)`
   - `perf: stop <name> fps=... p95=... long=...`
   - `perf: trace <uri>`

## Non-Goals

- Kernel changes.
- Full OpenTelemetry / OTLP / remote export.
- Making `perfd` the global tracing authority (metricsd/logd handle general observability).

## Constraints / invariants (hard requirements)

- Bounded memory and bounded export size.
- Deterministic metric computation given a deterministic stream of inputs.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

New deterministic host tests (suggested: `tests/perf_v1_host/`):

- feed a fixed sequence of `frameTick` events → metrics are deterministic
- begin/end spans serialize into a stable Chrome Trace JSON ordering (golden hash)
- export size bounded and ring truncation deterministic

### Proof (OS/QEMU) — gated

Once `/state` is available and perfd is wired:

- `perfd: ready` marker appears
- `perf: trace state://perf/traces/...` appears after an export trigger

## Touched paths (allowlist)

- `source/services/perfd/` (new)
- `tools/nexus-idl/schemas/perf.capnp` (new)
- `tests/perf_v1_host/` (new)
- `docs/perf/overview.md` (added in follow-up task or here if minimal)

## Plan (small PRs)

1. perfd core ring + metric computation + markers
2. chrome trace export (host-first), `/state` export once available
3. host tests
