# Next Task Preparation (Drift-Free)

<!--
CONTEXT
This file drives your "prep" ritual: validate the *next* task is drift-free
relative to current_state.md and the system vision/contracts before starting
a new chat/session.

It should be updated during the previous task's wrap-up, before handing off.
-->

## Candidate next task
- **task**: `tasks/TASK-0011B-kernel-rust-idioms-pre-smp.md`
- **handoff_target**: `.cursor/handoff/current.md` (always updated as the live entry-point)
- **handoff_archive**: `.cursor/handoff/archive/TASK-0009-persistence-v1-virtio-blk-statefs.md` (snapshot after completion)
- **linked_contracts**:
  - `docs/rfcs/RFC-0001-kernel-simplification.md` (kernel simplification measures; logic-preserving)
  - `docs/architecture/01-neuron-kernel.md` (kernel overview + invariants)
  - `docs/standards/RUST_STANDARDS.md` (kernel Rust idioms policy, pre-SMP guidance)
  - `scripts/qemu-test.sh` (canonical marker contract; must not change)
- **first_action**: Plan-first + touched-path allowlist enforcement (kernel protected zone)

## Drift-free check (must be YES to proceed)
- **aligns_with_current_state**: YES
  - TASK-0011 is complete (tree stabilized); TASK-0011B is the intended logic-preserving follow-up before SMP
- **best_system_solution**: YES
  - Lowest-risk pre-SMP prep: clarify ownership/Send-Sync boundaries and Rust idioms before behavioral SMP changes
- **scope_clear**: YES
  - Stop conditions explicit and marker-gated; no behavior/ABI/marker changes
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
  - Keep PRs small; be explicit about ownership invariants; markers must remain identical.
