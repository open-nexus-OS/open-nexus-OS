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
- **handoff_archive**: `.cursor/handoff/archive/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md` (latest completed-task snapshot, present)
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
  - `docs/testing/index.md` (global testing methodology)
  - `docs/testing/network-distributed-debugging.md` (SSOT for network/distributed triage + os2vm rule matrix)
  - `scripts/qemu-test.sh` (single-VM marker contract)
  - `tools/os2vm.sh` (cross-VM proof contract)
- **first_action**: execute `TASK-0016` close-out using new `os2vm` phase/summary flow (`session` then `remote`) and sync RFC evidence.

## Start slice (now)
- **slice_name**: TASK-0016 proof close-out with typed `os2vm` diagnostics
- **target_file**: `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md`
- **must_cover**:
  - keep single-VM and cross-VM marker/wire semantics unchanged while adding `TASK-0016` behavior
  - preserve nonce-correlated reply handling and remote proxy deny-by-default behavior
  - keep `source/services/dsoftbusd/src/main.rs` thin (no orchestration regressions back into main)
  - use `tools/os2vm.sh` summary artifacts (`json` + `txt`) as primary cross-VM triage evidence
  - update RFC/task proof sections with typed failure/success evidence, not only raw timeout output

## Execution order
1. **TASK-0015**: complete
2. **TASK-0016**: active
3. **TASK-0017**: next after TASK-0016 closure

## Drift-free check (must be YES to proceed)
- **aligns_with_current_state**: YES
  - `TASK-0016` remains active and now has improved deterministic debugging/proof tooling
- **best_system_solution**: YES
  - phase-gated/typed `os2vm` and SSOT docs reduce timeout-only debugging and proof ambiguity
- **scope_clear**: YES
  - remote packagefs RO remains primary scope; harness/doc changes are now in place for proof quality
- **touched_paths_allowlist_present**: YES
  - task allows `source/services/dsoftbusd/**`, docs sync, and harness updates in `tools/os2vm.sh`

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
  - ✅ network/distributed debugging SSOT added (`docs/testing/network-distributed-debugging.md`)

## Decision
- **status**: READY/ACTIVE (`TASK-0016` execution continues with upgraded proof/triage tooling)
- **notes**:
  - keep follow-on changes additive and preserve marker/wire/security invariants
  - prefer `RUN_PHASE=session|remote` loops for faster deterministic debugging
  - keep RFC and task evidence synchronized with `os2vm` typed summaries
  
## Next Task Preparation (Drift-Free)

## Candidate next task
- **task**: finalize `TASK-0016` review package and commit scope.
- **handoff_target**: `.cursor/handoff/current.md`
- **linked_contracts**:
  - `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md`
  - `docs/rfcs/RFC-0028-dsoftbus-remote-packagefs-ro-v1.md`
  - `docs/testing/network-distributed-debugging.md`
  - `scripts/qemu-test.sh`
  - `scripts/run-qemu-rv64.sh`
  - `tools/os2vm.sh`
  - `Makefile`
  - `justfile`

## Start slice (now)
- **slice_name**: final consistency + release hygiene
- **must_cover**:
  - keep test surfaces green (`fmt-check`, `test-all`, `make build`, `make test`)
  - decide and document `make run` default timeout policy
  - remove stale-editor ambiguity by using build/test evidence as source of truth
  - keep protocol constants cleanly used (no dead-code suppression shortcuts)

## Drift-free check
- **aligns_with_current_state**: YES
- **scope_clear**: YES
- **touched_paths_allowlist_present**: YES (task + harness/docs sync paths only)

## Dependencies & blockers
- **blocked_by**: none hard
- **watch_items**:
  - rust-analyzer stale diagnostics after rapid refactors
  - timeout budget mismatch between `make run` defaults and full marker ladder

## Decision
- **status**: READY
- **first_action**: confirm/implement timeout policy for `make run` and then finalize commit.
