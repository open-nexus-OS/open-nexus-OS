# Next Task Preparation (Drift-Free)

<!--
CONTEXT
This file drives your "prep" ritual: validate the *next* task is drift-free
relative to current_state.md and the system vision/contracts before starting
a new chat/session.

It should be updated during the previous task's wrap-up, before handing off.
-->

## Candidate next task
- **task**: `tasks/TASK-0194-networking-v1b-os-devnet-gated-real-connect.md`
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
  - `docs/adr/0025-qemu-smoke-proof-gating.md`
  - `docs/adr/0026-network-address-profiles-and-validation.md`
  - `docs/architecture/network-address-matrix.md`
  - `docs/testing/index.md`
  - `docs/testing/network-distributed-debugging.md`
  - `scripts/qemu-test.sh`
  - `tools/os2vm.sh`
- **first_action**: map `TASK-0194` changes onto the stabilized netstackd seams (`facade/handlers/*`, `ipc/reply`, `facade/tcp`) and keep marker/wire + address-profile contracts explicit before coding.

## Start slice (now)
- **slice_name**: TASK-0194 bring-up slice (devnet real connect, gated)
- **target_file**: `tasks/TASK-0194-networking-v1b-os-devnet-gated-real-connect.md`
- **must_cover**:
  - keep `netstackd` wire compatibility and deterministic marker ordering
  - preserve honest-green semantics for success markers
  - preserve ownership boundaries from `TASK-0003` / `RFC-0006`
  - preserve address-profile authority from `docs/architecture/network-address-matrix.md` + `docs/adr/0026-network-address-profiles-and-validation.md`
  - keep bounded retry/failure policy explicit and deterministic
  - use host-first + QEMU proof gating (`cargo test`, `dep-gate`, `diag-os`, `test-os`, `os2vm`)

## Execution order
1. **TASK-0016**: archived handoff / closure context retained
2. **TASK-0016B**: structural + optimization slice implemented; proofs green
3. **TASK-0194 / TASK-0196 / TASK-0249**: follow-ons from stabilized `netstackd` seams

## Drift-free check (must be YES to proceed)
- **aligns_with_current_state**: YES
  - follow-on kickoff matches completed `TASK-0016B` state in `current_state.md`
- **best_system_solution**: YES
  - move to feature follow-ons only after completed modularization + hardening + proofs
- **scope_clear**: YES
  - behavior-preserving acceptance and only minimal follow-up if evidence requires
- **touched_paths_allowlist_present**: YES
  - task allowlist still constrains scope to `source/services/netstackd/**` plus minimal docs sync

## Header / follow-up hygiene
- **follow_ups_in_task_header**: YES
  - task header links networking baseline, MMIO boundary, bring-up relation, and future networking consumers
- **security_considerations_complete**: YES
  - threat model, invariants, DON'T DO, mitigations, and proof expectations are present

## Dependencies & blockers
- **blocked_by**: none hard for `TASK-0194` kickoff
- **prereqs_ready**: YES
  - ✅ `TASK-0003` owner/contract baseline remains enforced
  - ✅ `TASK-0010` MMIO authority constraints unchanged
  - ✅ `RFC-0029` and `TASK-0016B` marked complete with optimization re-proofs
  - ✅ address/profile governance synced (`network-address-matrix.md` + `ADR-0026`) and proofed (`test-os-dhcp-strict`, `os2vm`)
  - ✅ deterministic proof policy remains aligned (`scripts/qemu-test.sh`, `tools/os2vm.sh`)

## Decision
- **status**: READY
- **notes**:
  - `netstackd` seams are stable enough for downstream feature work
  - keep new behavior behind explicit gates and preserve deterministic proof contracts
  - keep RFC/task/SSOT evidence synchronized during follow-on execution
