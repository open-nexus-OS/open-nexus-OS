---
title: TASK-0041 Lock profiling v1 (userspace-first): contention/hold-time stats + bounded export hooks (Superseded 2026-08-14 — motivation consumed by ADR-0049)
status: Superseded
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

## Context

> **SUPERSEDED (2026-08-14, sub-80 roadmap).** The justification ("before we optimize SMP, we
> need visibility") was consumed by the SMP/BKL track: ADR-0049 is Accepted and implemented,
> and the kernel ships exactly this profiling one layer down — `source/kernel/neuron/src/core/
> trap/budgets.rs` (`record_bkl_wait`, `record_ecall_hold` with worst-hold syscall attribution,
> 4-bucket wait histogram) enforced as a boot gate (`KSELFTEST: bkl budget ok`, hard-required in
> `scripts/qemu-test.sh`). Measured win recorded in ADR-0049: 90.8 ms → ~6 ms wait. The thin
> userspace residual (instrumenting `nexus-sync::SpinLock` / the four host-side `parking_lot`
> users) is explicitly NOT funded — `parking_lot` is forbidden in the OS graph (RFC-0009) and
> the ledger never targeted `nexus-sync`. If service-level lock visibility is ever needed,
> seed a fresh S-sized `nexus-sync` instrumentation task instead. Do not execute.

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
