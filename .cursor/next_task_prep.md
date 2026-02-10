# Next Task Preparation (Drift-Free)

<!--
CONTEXT
This file drives your "prep" ritual: validate the *next* task is drift-free
relative to current_state.md and the system vision/contracts before starting
a new chat/session.

It should be updated during the previous task's wrap-up, before handing off.
-->

## Candidate next task
- **task**: `tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md` (**NEXT**)
- **handoff_target**: `.cursor/handoff/current.md` (always updated as the live entry-point)
- **handoff_archive**: `.cursor/handoff/archive/TASK-0009-persistence-v1-virtio-blk-statefs.md` (snapshot after completion)
- **linked_contracts**:
  - `docs/rfcs/RFC-0020-kernel-ownership-and-rust-idioms-pre-smp-v1.md` (completed pre-SMP ownership/types baseline)
  - `docs/rfcs/RFC-0001-kernel-simplification.md` (background: layout/taxonomy baseline)
  - `docs/architecture/01-neuron-kernel.md` (kernel overview + invariants)
  - `docs/standards/RUST_STANDARDS.md` (kernel Rust idioms policy, pre-SMP guidance)
  - `scripts/qemu-test.sh` (canonical marker contract; must not change)
- **first_action**: Start TASK-0012 Phase 1 bootstrap using TASK-0011B ownership/safety markers as the SMP split baseline

## Start slice (now)
- **slice_name**: TASK-0012 Phase 1 — per-CPU scheduler scaffolding
- **target_file**: `source/kernel/neuron/src/sched/mod.rs` (and closely related kernel scheduler/task entry points)
- **must_cover**:
  - preserve marker/ABI semantics while introducing per-CPU-ready scheduler ownership points
  - keep deterministic bounded behavior in scheduling paths
  - reuse TASK-0011B typed handles + transfer/error envelopes (no rollback)

## Execution order
1. **TASK-0011B**: complete (Phases 0→5, proofs green)
2. **TASK-0012**: start per its SSOT execution slices

## Drift-free check (must be YES to proceed)
- **aligns_with_current_state**: YES
  - TASK-0011B is complete; TASK-0012 is the intended behavioral SMP follow-up
- **best_system_solution**: YES
  - TASK-0011B established pre-SMP contracts; TASK-0012 can now safely introduce per-CPU behavior
- **scope_clear**: YES
  - Transition from logic-preserving prep to scoped behavioral SMP changes under TASK-0012 proof gates
- **touched_paths_allowlist_present**: YES
  - Task declares allowlist (kernel + docs; verify before edits)

## Header / follow-up hygiene
- **follow_ups_in_task_header**: YES
  - Follow-up tasks are referenced in the task/RFC; do not start them until TASK-0011 is complete.
- **security_considerations_complete**: YES
  - Phase A is text-only; security invariants are documented, not modified

## Dependencies & blockers
- **blocked_by**: NONE
- **prereqs_ready**: YES
  - ✅ Kernel boots and marker contract is stable
  - ✅ Task defines strict “no behavior change” constraints

## Decision
- **status**: GO
- **notes**:
  - Keep PRs small; preserve marker/ABI contracts while introducing SMP behavior incrementally under TASK-0012.
