# Current Handoff: TASK-0012 Kernel SMP v1 (per-CPU runqueues + IPIs) — COMPLETE

**Date**: 2026-02-10  
**Status**: Completed with deterministic anti-fake evidence and negative SMP tests

---

## Completed in this slice

- **Execution SSOT**: `tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md` moved to `status: Complete`
- **Contract sync**: `docs/rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md` moved to `Status: Complete`
- **Harness gate**: SMP proof path now explicitly requires `REQUIRE_SMP=1` for SMP marker ladder
- **Anti-fake IPI chain**:
  - strict success chain required: `request accepted -> send_ipi ok -> S_SOFT trap observed -> ack`
  - deterministic counterfactual marker added: `KSELFTEST: ipi counterfactual ok`
- **Required negative tests (`test_reject_*`) added and marker-gated**:
  - `test_reject_invalid_ipi_target_cpu`
  - `test_reject_offline_cpu_resched`
  - `test_reject_steal_above_bound`
  - `test_reject_steal_higher_qos`

## Proofs run (sequential, green)

- `cargo test --workspace`
- `just dep-gate`
- `just diag-os`
- `SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- `SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`

## Exported guarantees to follow-ups

- **`TASK-0013`** (`tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md`):
  - may consume stable `QosClass` baseline and SMP marker ladder; must not bypass SMP ownership rules.
- **`TASK-0042`**:
  - may extend affinity/shares only on top of TASK-0012 bounded-steal and anti-fake IPI invariants.
- **`TASK-0247`**:
  - may harden RISC-V HSM/IPI/timer specifics; must not introduce a parallel SMP authority.
- **`TASK-0283`**:
  - `PerCpu<T>` adoption is a hardening refinement, not a semantic replacement of TASK-0012 behavior proofs.

## Next task entry-point

- Start `tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md`
- Keep `scripts/qemu-test.sh` SMP marker contract unchanged unless task explicitly updates docs/testing + task stop conditions in same slice.
