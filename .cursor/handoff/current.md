# Current Handoff: TASK-0018 crashdumps v1 implementation

**Date**: 2026-03-26  
**Status**: implementation + proofs complete, Phase 3 (strict child-write + drift lock) documented, pending commit.  
**Contract baseline**: `tasks/TASK-0018-crashdumps-v1-minidump-host-symbolize.md` (`In Review`)

---

## What is stable now

- `TASK-0017` closeout is archived:
  - `.cursor/handoff/archive/TASK-0017-dsoftbus-remote-statefs-rw.md`
- `userspace/crash` now provides deterministic bounded v1 dump framing + path normalization and required reject-path tests.
- `execd` crash flow now publishes deterministic crash metadata (`build_id`, `dump_path`) and enforces fail-closed publish authorization.
- `selftest-client` now writes/verifies crash dump artifacts, publishes crash metadata, and proves `SELFTEST: minidump ok`.
- strict child-owned write path is now proven end-to-end:
  - child payload writes `/state/crash/child.demo.minidump.nmd`,
  - `selftest-client` grants child `statefs` caps and reports located artifact metadata.
- minidump marker honesty is hardened: write path includes read-back + decode verification before success markers.
- `scripts/qemu-test.sh` marker ladder now includes `execd: minidump written` and `SELFTEST: minidump ok`.
- Host symbolization proof crate `tools/minidump-host` added and green.

## Current focus

- Keep TASK-0018 docs/SSOT in sync with proof evidence and preserve scope boundaries for follow-ons.

## Relevant contracts and linked work

- Active task:
  - `tasks/TASK-0018-crashdumps-v1-minidump-host-symbolize.md`
- RFC baseline:
  - `docs/rfcs/RFC-0031-crashdumps-v1-minidump-host-symbolize.md`
- Dependency contracts:
  - `tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md`
  - `tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md`
  - `docs/rfcs/RFC-0011-logd-journal-crash-v1.md`
  - `docs/rfcs/RFC-0018-statefs-journal-format-v1.md`
- Testing contract:
  - `scripts/qemu-test.sh`
  - `docs/testing/index.md`

## Immediate next slice

1. Commit the Phase 3 completion slice.
2. Open a dedicated identity-hardening follow-up slice (remove v1 proof-path subject mapping once child service identity is available).
3. Keep follow-on scopes (`TASK-0048`, `TASK-0049`, `TASK-0141`, `TASK-0142`, `TASK-0227`) out of this slice.

## Guardrails

- Keep kernel untouched.
- Keep v1 capture/publish in-process only; no cross-process post-mortem claims.
- Emit success markers only after real dump write/event publication.
- Keep artifact size/path validation bounded and deterministic.
- Maintain RFC/task progressive sync while implementation advances.

## Proof snapshot

- `cargo test -p crash -- --nocapture` ✅
- `cargo test -p minidump-host -- --nocapture` ✅
- `cargo test -p execd -- --nocapture` ✅
- `just dep-gate` ✅
- `just diag-os` ✅
- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` ✅
