# Next Task Preparation (Drift-Free)

<!--
CONTEXT
This file drives your "prep" ritual: validate the *next* task is drift-free
relative to current_state.md and the system vision/contracts before starting
a new chat/session.

It should be updated during the previous task's wrap-up, before handing off.
-->

## Candidate next task
- **task**: `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md`
- **handoff_target**: `.cursor/handoff/current.md` (always updated as the live entry-point)
- **handoff_archive**: `.cursor/handoff/archive/TASK-0014-observability-v2-metrics-tracing.md` (latest completed-task snapshot, present)
- **linked_contracts**:
  - `tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md`
  - `tasks/TASK-0003B-dsoftbus-noise-xk-os.md`
  - `tasks/TASK-0003C-dsoftbus-udp-discovery-os.md`
  - `tasks/TASK-0004-networking-dhcp-icmp-dsoftbus-dual-node.md`
  - `tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md`
  - `tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md` (execution SSOT + stop conditions)
  - `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md`
  - `tasks/TASK-0017-dsoftbus-remote-statefs-rw.md`
  - `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md`
  - `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`
  - `tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md`
  - `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md` (contract seed for the daemon refactor boundary)
  - `docs/adr/0005-dsoftbus-architecture.md` (service boundary / backend architecture)
  - `docs/distributed/dsoftbus-lite.md` (current daemon/backends overview)
  - `docs/testing/index.md` (proof sequencing + DSoftBus testing discipline)
  - `scripts/qemu-test.sh` (single-VM marker contract)
  - `tools/os2vm.sh` (cross-VM proof contract)
- **first_action**: start `TASK-0016` using the now-stabilized `dsoftbusd` seams from completed `TASK-0015`, without changing established marker/wire contracts.

## Start slice (now)
- **slice_name**: TASK-0016 kickoff / remote packagefs RO over DSoftBus seams
- **target_file**: `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md`
- **must_cover**:
  - keep single-VM and cross-VM marker/wire semantics unchanged while adding `TASK-0016` behavior
  - preserve nonce-correlated reply handling and remote proxy deny-by-default behavior
  - keep `source/services/dsoftbusd/src/main.rs` thin (no orchestration regressions back into main)
  - run host-first and sequential QEMU proofs after substantial changes

## Execution order
1. **TASK-0014**: complete
2. **TASK-0015**: complete
3. **TASK-0016**: next

## Drift-free check (must be YES to proceed)
- **aligns_with_current_state**: YES
  - task order is synced through completed `TASK-0015`; `TASK-0016` is the next explicit slice
- **best_system_solution**: YES
  - preparatory daemon modularization is complete and provides stable seams for remote packagefs work
- **scope_clear**: YES
  - next task scope is explicit and can be executed without reopening RFC-0027 Phase 3 refactor work
- **touched_paths_allowlist_present**: YES
  - task limits edits to `source/services/dsoftbusd/**`, docs sync, and harness-only `tools/os2vm.sh` parity updates

## Header / follow-up hygiene
- **follow_ups_in_task_header**: YES
  - task header links baseline dependencies (`TASK-0003*`, `TASK-0004`, `TASK-0005`) and all DSoftBus follow-ons (`TASK-0016`, `TASK-0017`, `TASK-0020`, `TASK-0021`, `TASK-0022`)
- **security_considerations_complete**: YES
  - threat model, invariants, DON'T DO, mitigations, and proof expectations are present

## Dependencies & blockers
- **blocked_by**: none
- **prereqs_ready**: YES
  - ✅ `TASK-0005` completed (cross-VM DSoftBus baseline)
  - ✅ `TASK-0014` completed and archived
  - ✅ deterministic proof policy remains aligned (`scripts/qemu-test.sh`, `tools/os2vm.sh`)

## Decision
- **status**: READY (`TASK-0015` complete; prep synced for `TASK-0016` kickoff)
- **notes**:
  - keep follow-on changes additive and preserve established marker/wire/security invariants
  - re-run host-first + sequential QEMU proofs after substantial changes
  - avoid reintroducing large orchestration loops into `main.rs`
