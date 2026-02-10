# Next Task Preparation (Drift-Free)

<!--
CONTEXT
This file drives your "prep" ritual: validate the *next* task is drift-free
relative to current_state.md and the system vision/contracts before starting
a new chat/session.

It should be updated during the previous task's wrap-up, before handing off.
-->

## Candidate next task
- **task**: `tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md` (**NEXT / ACTIVE PREP**)
- **handoff_target**: `.cursor/handoff/current.md` (always updated as the live entry-point)
- **handoff_archive**: `.cursor/handoff/archive/TASK-0011B-kernel-rust-idioms-pre-smp.md` (latest completed-task snapshot)
- **linked_contracts**:
  - `docs/rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md` (completed SMP v1 baseline contract)
  - `tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md` (execution SSOT, now complete)
  - `tasks/TASK-0277-kernel-smp-parallelism-policy-v1-deterministic.md` (normative SMP policy)
  - `docs/architecture/01-neuron-kernel.md` (ownership split + scheduler model)
  - `docs/adr/0025-qemu-smoke-proof-gating.md` (deterministic QEMU proof policy)
  - `docs/dev/platform/qemu-virtio-mmio-modern.md` (modern MMIO determinism floor)
  - `scripts/qemu-test.sh` (canonical marker contract; SMP markers must be explicitly gated)
- **first_action**: Start TASK-0013 QoS ABI/timed-coalescing slice using TASK-0012 SMP guarantees as a fixed baseline.

## Start slice (now)
- **slice_name**: TASK-0013 Phase 1 — QoS ABI contract + syscall plumbing (without new SMP authority)
- **target_file**: `source/kernel/neuron/src/sched/mod.rs` (plus syscall/task ABI surfaces touched by TASK-0013)
- **must_cover**:
  - preserve TASK-0012 marker and anti-fake invariants (no regression in SMP ladder)
  - keep deterministic bounded behavior in scheduler and QoS syscall paths
  - avoid parallel scheduler authority (TASK-0013 consumes TASK-0012, does not redefine it)

## Execution order
1. **TASK-0011B**: complete (phases 0→5, proofs green, archived handoff snapshot)
2. **TASK-0012**: complete (SMP baseline + anti-fake proof markers + `test_reject_*`)
3. **TASK-0013**: only after TASK-0012 stop conditions are green

## Drift-free check (must be YES to proceed)
- **aligns_with_current_state**: YES
  - TASK-0012 is complete and exports deterministic SMP guarantees required by TASK-0013
- **best_system_solution**: YES
  - QoS ABI/timer policy can now build on already-proven SMP baseline without contract drift
- **scope_clear**: YES
  - transition from SMP baseline to QoS ABI layer is explicit and bounded
- **touched_paths_allowlist_present**: YES
  - TASK-0012 allowlist includes kernel + harness + required docs sync paths

## Header / follow-up hygiene
- **follow_ups_in_task_header**: YES
  - TASK-0012 header exports explicit prerequisites for TASK-0013/0042/0247/0283
- **security_considerations_complete**: YES
  - TASK-0012 threat model/invariants are complete; preserve per-CPU isolation and bounded stealing guarantees

## Dependencies & blockers
- **blocked_by**: NONE
- **prereqs_ready**: YES
  - ✅ TASK-0011B complete and archived (`.cursor/handoff/archive/TASK-0011B-kernel-rust-idioms-pre-smp.md`)
  - ✅ TASK-0012 complete (strict SMP markers + anti-fake counterfactual + `test_reject_*` negatives)
  - ✅ modern MMIO/determinism policy aligned with harness/docs contracts

## Decision
- **status**: GO
- **notes**:
  - Keep TASK-0013 slices small and deterministic.
  - Preserve TASK-0012 SMP proof commands and marker semantics unless explicitly revised in-task with synchronized docs.
