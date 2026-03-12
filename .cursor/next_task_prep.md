# Next Task Preparation (Drift-Free)

<!--
CONTEXT
This file drives your "prep" ritual: validate the *next* task is drift-free
relative to current_state.md and the system vision/contracts before starting
a new chat/session.

It should be updated during the previous task's wrap-up, before handing off.
-->

## Candidate next task
- **task**: `tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md`
- **handoff_target**: `.cursor/handoff/current.md` (always updated as the live entry-point)
- **handoff_archive**: `.cursor/handoff/archive/TASK-0014-observability-v2-metrics-tracing.md` (latest completed-task snapshot, present)
- **linked_contracts**:
  - `tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md` (execution SSOT + stop conditions)
  - `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md` (contract seed for the daemon refactor boundary)
  - `docs/adr/0005-dsoftbus-architecture.md` (service boundary / backend architecture)
  - `docs/distributed/dsoftbus-lite.md` (current daemon/backends overview)
  - `scripts/qemu-test.sh` (single-VM marker contract)
  - `tools/os2vm.sh` (cross-VM proof contract)
- **first_action**: inspect `source/services/dsoftbusd/src/main.rs`, confirm extraction seams, and start the first behavior-preserving module split.

## Start slice (now)
- **slice_name**: TASK-0015 preparation / first refactor slice
- **target_file**: `source/services/dsoftbusd/src/main.rs`
- **must_cover**:
  - keep single-VM and cross-VM behavior unchanged
  - preserve marker names, marker timing semantics, and bounded retry loops
  - preserve nonce-correlated reply handling and remote proxy deny-by-default behavior
  - create seams for netstack IPC, discovery, session, gateway, and observability code

## Execution order
1. **TASK-0014**: complete
2. **TASK-0015**: ready to start

## Drift-free check (must be YES to proceed)
- **aligns_with_current_state**: YES
  - task order is synced through `TASK-0014`; `TASK-0015` is the next explicit slice
- **best_system_solution**: YES
  - preparatory daemon modularization reduces risk before new DSoftBus features land
- **scope_clear**: YES
  - task explicitly forbids new features, protocol changes, and shared-core extraction
- **touched_paths_allowlist_present**: YES
  - task limits edits to `source/services/dsoftbusd/**` plus narrow docs sync

## Header / follow-up hygiene
- **follow_ups_in_task_header**: YES
  - task header links the immediate DSoftBus follow-ons (`TASK-0016`, `TASK-0020`, `TASK-0021`, `TASK-0022`)
- **security_considerations_complete**: YES
  - threat model, invariants, DON'T DO, mitigations, and proof expectations are present

## Dependencies & blockers
- **blocked_by**: none
- **prereqs_ready**: YES
  - ✅ `TASK-0005` completed (cross-VM DSoftBus baseline)
  - ✅ `TASK-0014` completed and archived
  - ✅ deterministic proof policy remains aligned (`scripts/qemu-test.sh`, `tools/os2vm.sh`)

## Decision
- **status**: ACTIVE (`TASK-0015` is now in progress; preparation files synced)
- **notes**:
  - keep the task strictly refactor-only
  - prefer one extraction seam at a time and recheck proofs after each substantial split
