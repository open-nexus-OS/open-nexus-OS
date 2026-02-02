# Next Task Preparation (Drift-Free)

<!--
CONTEXT
This file drives your "prep" ritual: validate the *next* task is drift-free
relative to current_state.md and the system vision/contracts before starting
a new chat/session.

It should be updated during the previous task's wrap-up, before handing off.
-->

## Candidate next task
- **task**: `tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md`
- **handoff_target**: `.cursor/handoff/current.md` (always updated as the live entry-point)
- **handoff_archive**: `.cursor/handoff/archive/TASK-0010-device-mmio-access-v1.md` (snapshot after completion)
- **linked_contracts**:
  - `docs/rfcs/RFC-0017-device-mmio-access-model-v1.md` (Done - MMIO capability model)
  - **NEW RFC to create**: statefs journal format (RFC-00XX, use RFC-TEMPLATE.md + update README.md)
  - `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md` (capability distribution)
  - `docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md` (policy enforcement)
- **first_action**: Create RFC seed before implementation (Status: Draft)

## Drift-free check (must be YES to proceed)
- **aligns_with_current_state**: YES
  - TASK-0010 unblocked virtio-blk MMIO access (proven: `virtioblkd: mmio window mapped ok`)
  - Policy-gated distribution proven (`SELFTEST: mmio policy deny ok`)
  - All prerequisites (TASK-0008B entropy, TASK-0006 audit) are Done
- **best_system_solution**: YES
  - Userspace block device + journaled KV store (statefs) = kernel minimal
  - `/state` as dedicated authority (not VFS mount) = correct v1 scope
  - Host-first testing (BlockDevice trait + mem backend) = fast feedback
- **scope_clear**: YES
  - Stop conditions explicit: host tests + QEMU markers defined
  - Non-goals explicit: no POSIX semantics, no real reboot/VM reset in v1
  - Security considerations complete (CRC integrity, policy-gated access, negative tests required)
- **touched_paths_allowlist_present**: YES
  - Task declares allowlist (drivers/storage/virtio-blk, services/statefsd, userspace/statefs, etc.)

## Header / follow-up hygiene
- **follow_ups_in_task_header**: YES
  - Follow-up tasks listed: TASK-0034 (delta updates), TASK-0130 (packages), TASK-0018 (crashdumps), etc.
- **security_considerations_complete**: YES
  - Threat model: credential theft, tampering, journal corruption, unauthorized access
  - Required negative tests: `test_reject_corrupted_journal`, `test_reject_unauthorized_keystore_access`, etc.
  - Hardening markers defined: `statefsd: access denied`, `statefsd: crc mismatch`

## Dependencies & blockers
- **blocked_by**: NONE
  - TASK-0010 (MMIO access) is Done ✅
  - virtio-blk MMIO capability distribution proven in QEMU
  - `virtioblkd` service scaffold exists and proven
- **prereqs_ready**: YES
  - ✅ virtio-blk driver scaffold: `source/drivers/storage/virtio-blk/`
  - ✅ MMIO capability model: RFC-0017 Done
  - ✅ Policy + audit infrastructure: policyd + logd ready
  - ✅ Entropy authority: TASK-0008B Done (rngd → keystored)
  - ✅ QEMU marker harness: `scripts/qemu-test.sh` supports phased testing

## Decision
- **status**: GO
- **notes**:
  - virtio-blk MMIO access proven (`virtioblkd: mmio window mapped ok` marker green)
  - Policy deny-by-default proven (`SELFTEST: mmio policy deny ok` marker green)
  - All TASK-0010 tests green (just test-all, make test, make build, make run)
  - Ready to implement statefs journal engine (host-first) and integrate with block backend
  - "Soft reboot" proof defined: restart statefsd + replay journal, then verify persistence
