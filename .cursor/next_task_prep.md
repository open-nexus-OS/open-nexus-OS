# Next Task Preparation (Drift-Free)

<!--
CONTEXT
This file drives your "prep" ritual: validate the *next* task is drift-free
relative to current_state.md and the system vision/contracts before starting
a new chat/session.

It should be updated during the previous task's wrap-up, before handing off.
-->

## Candidate next task
- **task**: `tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md` (**NEXT / ACTIVE PREP**)
- **handoff_target**: `.cursor/handoff/current.md` (always updated as the live entry-point)
- **handoff_archive**: `.cursor/handoff/archive/TASK-0011B-kernel-rust-idioms-pre-smp.md` (latest completed-task snapshot)
- **linked_contracts**:
  - `docs/rfcs/RFC-0020-kernel-ownership-and-rust-idioms-pre-smp-v1.md` (completed pre-SMP ownership/types baseline)
  - `tasks/TASK-0277-kernel-smp-parallelism-policy-v1-deterministic.md` (normative SMP policy)
  - `docs/architecture/01-neuron-kernel.md` (ownership split + scheduler model)
  - `docs/adr/0025-qemu-smoke-proof-gating.md` (deterministic QEMU proof policy)
  - `docs/dev/platform/qemu-virtio-mmio-modern.md` (modern MMIO determinism floor)
  - `scripts/qemu-test.sh` (canonical marker contract; SMP markers must be explicitly gated)
- **first_action**: Start TASK-0012 Phase 1 bootstrap using TASK-0011B ownership/safety markers as the SMP split baseline.

## Start slice (now)
- **slice_name**: TASK-0012 Phase 1 — CPU discovery + online-mask scaffolding
- **target_file**: `source/kernel/neuron/src/sched/mod.rs` (plus tightly-coupled kernel scheduler/task entry points)
- **must_cover**:
  - preserve marker/ABI semantics while introducing per-CPU-ready scheduler ownership points
  - keep deterministic bounded behavior in scheduling and bring-up paths
  - reuse TASK-0011B typed handles + ownership/error-envelope discipline (no rollback)

## Execution order
1. **TASK-0011B**: complete (phases 0→5, proofs green, archived handoff snapshot)
2. **TASK-0012**: active prep now, implement per SSOT slices
3. **TASK-0013**: only after TASK-0012 stop conditions are green

## Drift-free check (must be YES to proceed)
- **aligns_with_current_state**: YES
  - TASK-0011B is complete; TASK-0012 is the intended behavioral SMP follow-up
- **best_system_solution**: YES
  - pre-SMP contracts are complete, so per-CPU behavior can now be introduced with explicit ownership boundaries
- **scope_clear**: YES
  - transition from logic-preserving prep to scoped behavioral SMP changes under TASK-0012 proof gates
- **touched_paths_allowlist_present**: YES
  - TASK-0012 allowlist includes kernel + harness + required docs sync paths

## Header / follow-up hygiene
- **follow_ups_in_task_header**: YES
  - TASK-0012 header now carries explicit `enables` + `follow-up-tasks` boundaries (anti-drift)
- **security_considerations_complete**: YES
  - TASK-0012 threat model/invariants are complete; preserve per-CPU isolation and bounded stealing guarantees

## Dependencies & blockers
- **blocked_by**: NONE
- **prereqs_ready**: YES
  - ✅ TASK-0011B complete and archived (`.cursor/handoff/archive/TASK-0011B-kernel-rust-idioms-pre-smp.md`)
  - ✅ TASK-0012 contract sync done (RED decision resolved, anti-drift boundaries explicit)
  - ✅ modern MMIO/determinism policy aligned with harness/docs contracts

## Decision
- **status**: GO
- **notes**:
  - Keep TASK-0012 implementation slices small and deterministic.
  - Enforce SMP=2 and SMP=1 proof runs explicitly; preserve default single-hart smoke behavior.
