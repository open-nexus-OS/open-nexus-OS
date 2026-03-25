# Current Handoff: TASK-0017 remote statefs RW kickoff (prep)

**Date**: 2026-03-24  
**Status**: prepare `TASK-0017` kickoff; scope and contract set to draft baseline.  
**Contract baseline**: `tasks/TASK-0017-dsoftbus-remote-statefs-rw.md` (`Draft`)

---

## What is stable now

- DSoftBus modular daemon seams from `TASK-0015` are in place (`gateway/session/observability` split, thin main wiring).
- Remote packagefs RO path from `TASK-0016` is complete and can be reused as transport/reference shape.
- `netstackd` modularization and deterministic networking proof hardening from `TASK-0016B` are complete and green.
- Statefs baseline (`TASK-0009`) and policy/audit baseline (`TASK-0008`) are already available for this slice.
- Canonical proof harnesses are stable:
  - single VM: `scripts/qemu-test.sh`
  - two VM: `tools/os2vm.sh`

## Current focus

- Start `TASK-0017`: remote statefs proxy over authenticated DSoftBus streams with RW ACL enforcement and deterministic audit evidence.

## Relevant contracts and linked work

- Active task:
  - `tasks/TASK-0017-dsoftbus-remote-statefs-rw.md`
- Required prerequisites:
  - `tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md`
  - `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md`
  - `tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md`
  - `tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md`
  - `tasks/TASK-0008-security-hardening-v1-nexus-sel-audit-device-keys.md`
  - `tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md`
- RFC / ADR baseline:
  - `docs/rfcs/RFC-0030-dsoftbus-remote-statefs-rw-v1.md`
  - `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`
  - `docs/adr/0005-dsoftbus-architecture.md`
- Testing contracts:
  - `docs/testing/index.md`
  - `scripts/qemu-test.sh`
  - `tools/os2vm.sh`

## Immediate next slice

1. Define the minimal remote-statefs RW protocol boundary and ACL checks at gateway level.
2. Add host-first behavior tests, especially required negative cases:
   - `test_reject_statefs_write_outside_acl`
   - `test_reject_statefs_prefix_escape`
   - `test_reject_oversize_statefs_write`
   - `test_reject_unauthenticated_statefs_request`
3. Add deterministic proof markers and wire them into QEMU gates:
   - `dsoftbusd: remote statefs served`
   - `SELFTEST: remote statefs rw ok`

## Guardrails

- Keep scope to remote statefs proxy only; no generic remote filesystem expansion.
- Enforce ACL deny-by-default (`/state/shared/*` only for remote RW) with deterministic `EPERM` for violations.
- Emit audit evidence for every remote `PUT`/`DELETE` (logd-backed or deterministic fallback marker).
- Keep bounded key/value sizes and bounded retry loops; no unbounded transport/write loops.
- Keep kernel untouched.
- Keep proofs sequential and deterministic (`qemu-test.sh` then `os2vm.sh`).
