# Handoff Archive: TASK-0012B Kernel SMP v1b hardening

**Date**: 2026-02-11  
**Status**: Done

## What closed in TASK-0012B

- Execution SSOT: `tasks/TASK-0012B-kernel-smp-v1b-scheduler-smp-hardening.md` moved to `status: Done`.
- Contract sync: `docs/rfcs/RFC-0022-kernel-smp-v1b-scheduler-hardening-contract.md` moved to `Status: Done`.
- Scheduler hot-path hardening:
  - explicit bounded enqueue contract,
  - deterministic immediate reject semantics on queue saturation.
- Trap/IPI hardening:
  - explicit S_SOFT resched contract path,
  - preserved TASK-0012 marker semantics and anti-fake evidence chain.
- CPU-ID hardening:
  - guarded hybrid contract (`tp` hint -> stack-range fallback -> BOOT fallback).
- Ownership/error propagation hardening:
  - `#[must_use]` outcomes consumed explicitly for enqueue/wake/resched paths.
- Trap-runtime safety hardening:
  - mutable trap-runtime kernel handles are boot-hart-only in v1b; secondary-hart trap access fails closed.

## Proof commands (green)

- `cargo test --workspace`
- `just dep-gate`
- `just diag-os`
- `SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- `SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`

## Exported guarantees to follow-ups

- `TASK-0013`: may add QoS ABI/timed coalescing policy surface only; no SMP authority fork.
- `TASK-0042`: affinity/shares must extend the same scheduler authority and bounded contracts.
- `TASK-0247`: owns deferred per-hart trap runtime ownership, stack-overflow detection strategy, NMI safety policy, and FPU context policy.
- `TASK-0283`: `PerCpu<T>` remains a refinement layer on top of 0012/0012B guarantees.
