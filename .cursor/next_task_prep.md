# Next Task Preparation (Drift-Free)

<!--
CONTEXT
This file drives your "prep" ritual: validate the *next* task is drift-free
relative to current_state.md and the system vision/contracts before starting
a new chat/session.

It should be updated during the previous task's wrap-up, before handing off.
-->

## Candidate next task
- **task**: `tasks/TASK-0011-kernel-simplification-phase-a.md`
- **handoff_target**: `.cursor/handoff/current.md` (always updated as the live entry-point)
- **handoff_archive**: `.cursor/handoff/archive/TASK-0009-persistence-v1-virtio-blk-statefs.md` (snapshot after completion)
- **linked_contracts**:
  - `docs/rfcs/RFC-0001-kernel-simplification.md` (kernel simplification measures; logic-preserving)
  - `docs/architecture/01-neuron-kernel.md` (kernel overview + invariants)
  - `scripts/qemu-test.sh` (canonical marker contract; must not change)
- **first_action**: Plan-first + touched-path allowlist enforcement (kernel protected zone)

## Drift-free check (must be YES to proceed)
- **aligns_with_current_state**: YES
  - RFC-0001 explicitly delegates execution/proofs to TASK-0011
  - Task allowlist is present and scoped to kernel headers + docs
- **best_system_solution**: YES
  - Lowest-risk kernel clarity uplift: improve navigation/debuggability without changing behavior
- **scope_clear**: YES
  - Stop conditions explicit: QEMU markers unchanged; text-only change discipline
  - Non-goals explicit: no behavior/ABI/marker changes; no dependency changes
- **touched_paths_allowlist_present**: YES
  - Task declares allowlist (`source/kernel/neuron/src/**`, `docs/**`)

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
  - Keep PRs small, mechanical, and phase-scoped; markers must remain identical.
