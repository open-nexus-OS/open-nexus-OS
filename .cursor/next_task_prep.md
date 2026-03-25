# Next Task Preparation (Drift-Free)

<!--
CONTEXT
Preparation file for the next execution slice.
Update during wrap-up so a new session can start without drift.
-->

## Candidate next task
- **task**: `tasks/TASK-0017-dsoftbus-remote-statefs-rw.md`
- **handoff_target**: `.cursor/handoff/current.md`
- **handoff_archive**: `.cursor/handoff/archive/TASK-0016B-netstackd-modular-refactor-v1.md`
- **linked_contracts**:
  - `tasks/TASK-0017-dsoftbus-remote-statefs-rw.md`
  - `tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md`
  - `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md`
  - `tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md`
  - `tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md`
  - `tasks/TASK-0008-security-hardening-v1-nexus-sel-audit-device-keys.md`
  - `tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md`
  - `docs/rfcs/RFC-0030-dsoftbus-remote-statefs-rw-v1.md`
  - `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`
  - `docs/adr/0005-dsoftbus-architecture.md`
  - `docs/testing/index.md`
  - `docs/testing/network-distributed-debugging.md`
  - `scripts/qemu-test.sh`
  - `tools/os2vm.sh`
- **first_action**: define minimal remote-statefs RW protocol + ACL/audit invariants and seed host negative tests before transport-side expansion.

## Start slice (now)
- **slice_name**: TASK-0017 remote statefs RW ACL/audit kickoff
- **target_file**: `tasks/TASK-0017-dsoftbus-remote-statefs-rw.md`
- **must_cover**:
  - authenticated peer-only remote RW
  - deny-by-default ACL (`/state/shared/*` only)
  - deterministic fail-closed rejects (`EPERM`/bounds/auth)
  - audit evidence for each remote `PUT`/`DELETE`
  - bounded retries and deterministic proof markers

## Execution order
1. **TASK-0016**: remote packagefs RO (Done)
2. **TASK-0016B**: netstackd modularization + hardening (Done)
3. **TASK-0017**: remote statefs RW ACL/audit (current prep target)
4. **TASK-0020 / TASK-0021 / TASK-0022**: transport and core follow-ons (not in scope now)

## Drift-free check (must be YES to proceed)
- **aligns_with_current_state**: YES
  - current SSOT now points at TASK-0017 kickoff prep
- **best_system_solution**: YES
  - leverage existing modular seams and proven harnesses before new transport complexity
- **scope_clear**: YES
  - task remains proxy-level RW ACL/audit work, not full distributed storage redesign
- **touched_paths_allowlist_present**: YES
  - task allowlist constrained to dsoftbusd/statefs/selftest/harness/docs paths

## Header / follow-up hygiene
- **follow_ups_in_task_header**: YES
  - TASK-0020, TASK-0021, TASK-0022 are explicitly linked as follow-ons
- **security_considerations_complete**: YES
  - threat model, invariants, DON'T DO, and required negative tests are explicit

## Dependencies & blockers
- **blocked_by**: none hard
- **prereqs_ready**: YES
  - `TASK-0015`, `TASK-0016`, `TASK-0016B` completed
  - `TASK-0005`, `TASK-0009`, `TASK-0008`, `TASK-0006` completed
  - single-VM and 2-VM harness contracts available

## Decision
- **status**: READY
- **notes**:
  - start host-first with fail-closed tests and ACL normalization checks
  - only then wire marker proof extensions into QEMU gates
