# Next Task Preparation (Drift-Free)

<!--
CONTEXT
This file drives your "prep" ritual: validate the *next* task is drift-free
relative to current_state.md and the system vision/contracts before starting
a new chat/session.

It should be updated during the previous task's wrap-up, before handing off.
-->

## Candidate next task
- **task**: `tasks/TASK-0016B-netstackd-refactor-v1-modular-os-daemon-structure.md`
- **handoff_target**: `.cursor/handoff/current.md` (always updated as the live entry-point)
- **handoff_archive**: `.cursor/handoff/archive/TASK-0016-remote-packagefs-ro.md` (latest archived handoff snapshot)
- **linked_contracts**:
  - `tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md`
  - `tasks/TASK-0010-device-mmio-access-model.md`
  - `tasks/TASK-0249-bringup-rv-virt-v1_2b-os-virtionetd-netstackd-fetchd-echod-selftests.md`
  - `tasks/TASK-0194-networking-v1b-os-devnet-gated-real-connect.md`
  - `tasks/TASK-0196-dsoftbus-v1_1b-devnet-udp-discovery-gated.md`
  - `docs/rfcs/RFC-0006-userspace-networking-v1.md`
  - `docs/rfcs/RFC-0017-device-mmio-access-model-v1.md`
  - `docs/rfcs/RFC-0029-netstackd-modular-daemon-structure-v1.md`
  - `docs/adr/0005-dsoftbus-architecture.md`
  - `docs/testing/index.md`
  - `docs/testing/network-distributed-debugging.md`
  - `scripts/qemu-test.sh`
  - `tools/os2vm.sh`
- **first_action**: start `TASK-0016B` Phase 0 by creating the `netstackd` internal `src/os/` scaffold and reducing `main.rs` to entry/wiring only.

## Start slice (now)
- **slice_name**: TASK-0016B Phase 0 scaffold + boundary extraction
- **target_file**: `tasks/TASK-0016B-netstackd-refactor-v1-modular-os-daemon-structure.md`
- **must_cover**:
  - keep `netstackd` marker and wire semantics unchanged during extraction
  - preserve ownership boundaries from `TASK-0003` / `RFC-0006`
  - keep bounded retry/failure policy explicit and deterministic
  - create narrow host-testable seams before claiming hardening coverage
  - keep the eventual hardening phase internal and behavior-preserving

## Execution order
1. **TASK-0016**: archived handoff / closure context retained
2. **TASK-0016B**: active next structural task
3. **TASK-0194 / TASK-0196 / TASK-0249**: follow-ons after stable `netstackd` seams exist

## Drift-free check (must be YES to proceed)
- **aligns_with_current_state**: YES
  - `TASK-0016B` matches the new active focus in `current_state.md`
- **best_system_solution**: YES
  - modularizing `netstackd` before more networking features reduces review risk and scope drift
- **scope_clear**: YES
  - structure-first, then bounded hardening, with no public-contract expansion
- **touched_paths_allowlist_present**: YES
  - task contains explicit allowlist for `source/services/netstackd/**` plus RFC/task/doc sync

## Header / follow-up hygiene
- **follow_ups_in_task_header**: YES
  - task header links networking baseline, MMIO boundary, bring-up relation, and future networking consumers
- **security_considerations_complete**: YES
  - threat model, invariants, DON'T DO, mitigations, and proof expectations are present

## Dependencies & blockers
- **blocked_by**: none hard for Phase 0 planning/refactor
- **prereqs_ready**: YES
  - ✅ `TASK-0003` defines the owner/contract baseline
  - ✅ `TASK-0010` defines MMIO authority constraints
  - ✅ `RFC-0029` seed now exists for the internal refactor boundary
  - ✅ deterministic proof policy remains aligned (`scripts/qemu-test.sh`, `tools/os2vm.sh`)

## Decision
- **status**: READY
- **notes**:
  - keep follow-on changes additive and preserve marker/wire/ownership invariants
  - use `TASK-0015` as the structural pattern, not as a feature template
  - keep RFC-0029 and task evidence synchronized as phases land
