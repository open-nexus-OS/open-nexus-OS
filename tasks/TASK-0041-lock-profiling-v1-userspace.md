---
title: TASK-0041 Lock profiling v1: contention/hold-time visibility (delivered as kernel lock budgets, ADR-0049)
status: Done (2026-09-09 — reconciled: visibility goal delivered by kernel lock budgets, ADR-0049)
owner: @runtime
created: 2025-12-22
depends-on:
  - TASK-0014
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Depends-on (metrics/tracing sinks, optional): tasks/TASK-0014-observability-v2-metrics-tracing.md
  - Testing contract: scripts/qemu-test.sh
---

## Closure 2026-09-09 (reconciliation — Done, delivered one layer down)

**Goal of this ledger:** lock contention/hold-time visibility *before* optimizing SMP, with
deterministic proof and bounded overhead.

**Actual solution (code ground truth, verified 2026-09-09):** the visibility landed in the
kernel, where the contention actually was, and it is a boot gate rather than an opt-in library:

- `source/kernel/neuron/src/core/trap/budgets.rs` — `record_bkl_wait`, `record_ecall_hold`
  (worst-hold syscall attribution, wait histogram), called from `core/trap/runtime.rs` and
  `core/trap/handler.rs`; reported by `syscall/api/sched_telemetry.rs` as
  `KSELFTEST: bkl budget ok (max_wait=…us max_hold=…ms nr=…)` and hard-required in
  `scripts/qemu-test.sh` (plus `runtime timer budget ok` / `runtime ipi budget ok`).
- Measured outcome recorded in `docs/adr/0049-bkl-lockclass-and-softrt-cpu-placement.md`:
  BKL wait 90.8 ms → ~6 ms.

**Not delivered / not needed:** a userspace `nexus-lockprof` crate. OS services are
single-threaded event loops on a bump allocator (no intra-service lock contention to profile),
`parking_lot` is forbidden in the OS graph (RFC-0009), and the host-side `parking_lot` users
are test/tooling code. No end-system consumer exists for service-level lock statistics.

## Context

Before we “optimize SMP”, we need visibility. A userspace lock profiler gives immediate value on host and
in OS services without requiring kernel changes:

- find contention hot spots,
- measure wait/hold times,
- provide actionable names and call sites.

## Goal

Deliver a lightweight lock profiling library that can instrument critical services and produce deterministic
proof (host tests), with optional export to metrics/tracing later.

## Non-Goals

- Kernel lock profiling.
- Perfect call stack unwinding or symbolization in v1.
- Mandatory dependency on metricsd/logd/traced (export is optional).

## Constraints / invariants (hard requirements)

- Kernel untouched.
- Overhead bounded and controllable (feature flag + sampling thresholds).
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Deterministic tests (injectable clock; avoid flaky wall-clock assertions).

## Red flags / decision points

- **YELLOW (call site identifiers)**:
  - `file:line` can be used as a best-effort identifier but may be optimized out; keep it optional.
- **YELLOW (async runtimes)**:
  - We should not pull in a full async runtime just to profile locks; support async mutexes only where already present.

## Stop conditions (Definition of Done)

### Proof (Host)

- New deterministic tests (`tests/lockprof_host/` or crate tests):
  - contention count increments
  - hold-time accounting increases under a synthetic workload
  - p95 thresholds trigger a “hot lock” event deterministically (using an injected clock).

### Proof (OS / QEMU) — optional later

Only once the OS services are instrumented:

- `lockprof: on`
- `SELFTEST: lock hot ok`

## Touched paths (allowlist)

- `userspace/diagnostics/` (new `nexus-lockprof` crate)
- `source/services/*/` (optional: instrument a few critical locks)
- `source/apps/selftest-client/` (optional OS markers)
- `docs/perf/lock-profiling.md`

## Plan (small PRs)

1. **Implement `nexus-lockprof`**
   - wrappers for `parking_lot::{Mutex,RwLock}` with names
   - stats: contention count, wait ns, hold ns, simple percentiles (p50/p95/p99 via fixed reservoir)
   - threshold events (lock.hot) with a stable marker string for tests.

2. **Host tests**
   - synthetic contention microbench; verify stats.

3. **Docs**
   - how to instrument locks, overhead knobs, interpretation guidance.
