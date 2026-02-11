# Current Handoff: TASK-0012B Kernel SMP v1b hardening — COMPLETE

**Date**: 2026-02-10  
**Status**: TASK-0012 and TASK-0012B closed; handoff moves to TASK-0013

---

## Baseline now fixed (from TASK-0012)

- `tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md` is `status: Done`.
- SMP proof ladder is canonical and must stay stable:
  - `SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - `SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- Anti-fake/negative markers are baseline contracts:
  - `KSELFTEST: ipi counterfactual ok`
  - `KSELFTEST: ipi resched ok`
  - `KSELFTEST: test_reject_invalid_ipi_target_cpu ok`
  - `KSELFTEST: test_reject_offline_cpu_resched ok`
  - `KSELFTEST: test_reject_steal_above_bound ok`
  - `KSELFTEST: test_reject_steal_higher_qos ok`

## Completed focus (TASK-0012B)

- Completed task: `tasks/TASK-0012B-kernel-smp-v1b-scheduler-smp-hardening.md`
- Landed:
  - explicit bounded scheduler enqueue contract with deterministic reject semantics,
  - explicit S_SOFT resched contract path while preserving TASK-0012 marker semantics,
  - guarded CPU-ID hybrid path (`tp` hint -> stack-range fallback -> BOOT fallback).
- Kept unchanged:
  - TASK-0012 marker semantics and anti-fake causal chain.
  - SMP proof command shape (`SMP=2` gated + `SMP=1` regression).

## Proof floor executed (TASK-0012B)

- `cargo test --workspace`
- `just dep-gate`
- `just diag-os`
- `SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- `SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`

## Immediate next entry-point

- Next task: `tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md`.
- Preserve TASK-0012/TASK-0012B authority: no alternate SMP authority path and no marker-semantics drift.
