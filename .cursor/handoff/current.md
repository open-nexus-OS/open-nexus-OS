# Current Handoff: TASK-0018 crashdumps v1 contract kickoff

**Date**: 2026-03-26  
**Status**: draft-prep active (task hardened, RFC seed created, implementation not started).  
**Contract baseline**: `tasks/TASK-0018-crashdumps-v1-minidump-host-symbolize.md` (`Draft`)

---

## What is stable now

- `TASK-0017` closeout is archived:
  - `.cursor/handoff/archive/TASK-0017-dsoftbus-remote-statefs-rw.md`
- `TASK-0018` was hardened to match repo discipline:
  - structured follow-up links in header,
  - explicit security section (threat model/invariants/DON'T DO),
  - RED item resolved for v1 scope (in-process capture only; no ptrace-like requirement),
  - host-vs-OS proof split clarified to avoid fake-green marker claims.
- RFC seed created:
  - `docs/rfcs/RFC-0031-crashdumps-v1-minidump-host-symbolize.md` (Draft)
  - indexed in `docs/rfcs/README.md`.

## Current focus

- Start TASK-0018 implementation slice strictly inside its touched-path allowlist.
- Keep contract-first behavior: bounded artifacts, deterministic event/marker semantics, host-first symbolization.

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

1. Prepare implementation plan against TASK-0018 acceptance/stop conditions.
2. Add v1 crashdump touched paths (`userspace/crash`, `execd`, `selftest-client`, host symbolization tooling/tests) incrementally.
3. Keep follow-on scopes (`TASK-0048`, `TASK-0049`, `TASK-0141`, `TASK-0227`) out of this implementation slice.

## Guardrails

- Keep kernel untouched.
- Keep v1 capture in-process only; no cross-process post-mortem claims.
- Emit success markers only after real dump write/event publication.
- Keep artifact size/path validation bounded and deterministic.
- Maintain RFC/task progressive sync while implementation advances.
