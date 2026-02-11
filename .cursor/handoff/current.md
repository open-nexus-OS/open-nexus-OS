# Current Handoff: TASK-0012B Kernel SMP v1b hardening — PREP/ACTIVE

**Date**: 2026-02-10  
**Status**: TASK-0012 closed; TASK-0012B is the active hardening bridge

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

## Active focus (TASK-0012B)

- Entry task: `tasks/TASK-0012B-kernel-smp-v1b-scheduler-smp-hardening.md`
- Scope:
  - bounded queue/backpressure contract in scheduler hot paths,
  - trap/IPI resched contract hardening without changing marker semantics,
  - CPU-ID fast-path + deterministic fallback contract.
- Do not introduce:
  - new userspace scheduler ABI (TASK-0013/0042 own this),
  - a second SMP authority path,
  - timing-based proof success logic.

## Proof floor for TASK-0012B slices

- `cargo test --workspace`
- `just dep-gate`
- `just diag-os`
- `SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- `SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`

## Immediate next entry-point

- Start with `source/kernel/neuron/src/sched/mod.rs` and `source/kernel/neuron/src/core/smp.rs`.
- Keep `scripts/qemu-test.sh` marker ordering/meaning unchanged unless docs + task contracts are synchronized in the same slice.
