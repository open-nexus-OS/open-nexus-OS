# Handoff Archive: TASK-0012 Kernel SMP v1 (per-CPU runqueues + IPIs)

**Date**: 2026-02-10  
**Status**: Complete

## What closed in TASK-0012

- Execution SSOT: `tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md` moved to `status: Done`.
- Contract sync: `docs/rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md` moved to `Status: Complete`.
- SMP proof gate hardened with explicit `REQUIRE_SMP=1`.
- Anti-fake IPI evidence chain is now required:
  - `request accepted -> send_ipi ok -> S_SOFT trap observed -> ack`.
- Deterministic counterfactual marker added:
  - `KSELFTEST: ipi counterfactual ok`.
- Required negative tests/markers:
  - `test_reject_invalid_ipi_target_cpu`
  - `test_reject_offline_cpu_resched`
  - `test_reject_steal_above_bound`
  - `test_reject_steal_higher_qos`

## Proof commands (green)

- `cargo test --workspace`
- `just dep-gate`
- `just diag-os`
- `SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- `SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`

## Exported guarantees to follow-ups

- `TASK-0013`: consumes stable QoSClass/SMP baseline; no alternate scheduler authority.
- `TASK-0042`: extends policy (affinity/shares) while preserving TASK-0012 invariants.
- `TASK-0247`: may harden RISC-V specifics, not redefine SMP authority.
- `TASK-0283`: `PerCpu<T>` is a hardening refinement only.
