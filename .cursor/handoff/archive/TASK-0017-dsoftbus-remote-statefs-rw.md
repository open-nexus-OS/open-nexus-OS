# Current Handoff: TASK-0017 remote statefs RW closeout

**Date**: 2026-03-25  
**Status**: complete (all task stop conditions green).  
**Contract baseline**: `tasks/TASK-0017-dsoftbus-remote-statefs-rw.md` (`Complete`)

---

## What is stable now

- `dsoftbusd` remote statefs gateway contract path is implemented (bounded v1 frame handling, auth/ACL/bounds checks, deterministic fail-closed rejects).
- Remote statefs path is bridged to `statefsd` (persistence parity closed) with deterministic nonce-correlated response matching.
- Required negative tests exist and pass:
  - `test_reject_statefs_write_outside_acl`
  - `test_reject_statefs_prefix_escape`
  - `test_reject_oversize_statefs_write`
  - `test_reject_unauthenticated_statefs_request`
- Marker evidence is present in QEMU proofs:
  - `dsoftbusd: remote statefs served`
  - `SELFTEST: remote statefs rw ok`
- Proof chain is green (sequential):
  - `cargo test -p dsoftbusd --tests -- --nocapture`
  - `just dep-gate`
  - `just diag-os`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
  - os2vm evidence: `artifacts/os2vm/runs/os2vm_1774454076/summary.json` + `.txt`

## Current focus

- Keep TASK-0017 closed and drift-free; do not pull follow-on transport scope into this slice.

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

1. Keep `TASK-0017` and `RFC-0030` at `Complete` unless contract/proof regressions appear.
2. Start only explicitly requested follow-on scope (`TASK-0020`/`TASK-0021`/`TASK-0022`).

## Guardrails

- Keep scope to remote statefs proxy only; no generic remote filesystem expansion.
- Enforce ACL deny-by-default (`/state/shared/*` only for remote RW) with deterministic `EPERM` for violations.
- Emit audit evidence for every remote `PUT`/`DELETE` (logd-backed or deterministic fallback marker).
- Keep bounded key/value sizes and bounded retry loops; no unbounded transport/write loops.
- Keep kernel untouched.
- Keep proofs sequential and deterministic (`qemu-test.sh` then `os2vm.sh`).
