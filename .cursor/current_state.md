# Cursor Current State (SSOT)

## Current architecture state

- **last_decision (2026-04-23)**: preparation focus moved to `TASK-0032` with task text aligned to real `packagefsd` baseline, explicit security reject contract, and explicit Gate-C production dependencies (`TASK-0033`, `TASK-0286`, `TASK-0287`, `TASK-0290`).
  - `TASK-0031` remains `In Review`; `RFC-0040` remains `Done` (scoped v1 plumbing closure).
- **prev_decision (2026-04-23)**: `TASK-0031` and `RFC-0040` are scoped/closed as v1 plumbing (host + OS proofs green; kernel production closure explicitly handed off).
  - `TASK-0029` and `RFC-0039` remain closed (`Done`) and are no longer active execution scope.
  - New seed RFC `RFC-0040` exists and is linked from `TASK-0031`.
  - RFC-0040 is `Done` for v1 plumbing scope (Phase 0/1 done + explicit out-of-scope handoff to `TASK-0290`).
  - `userspace/memory` (`nexus-vmo`) now exists with bounded API + deterministic counters + `test_reject_*` proofs.
  - `selftest-client` now emits `vmo: producer sent handle` -> `vmo: consumer mapped ok` -> `vmo: sha256 ok` -> `SELFTEST: vmo share ok` from a real producer/consumer task split (slot-directed transfer + consumer-side RO map verification).
  - `TASK-0031` is `In Review` as plumbing/honesty floor (host-first + OS-gated), while kernel-enforced seal/rights closure remains in `TASK-0290`.

- **older_decision (2026-04-22)**: `TASK-0029` closure remediation completed and status synchronized (`TASK-0029` + `RFC-0039` marked done).

## Active focus (execution)

- **active_task**: `tasks/TASK-0032-packagefs-v2-ro-image-index-fastpath.md` — `Draft`
- **immediate_follow_up**: `tasks/TASK-0033-packagefs-v2b-vmo-splice-from-image.md` — `Draft`
- **production_dependencies**: `TASK-0286` + `TASK-0287` + `TASK-0290` remain explicit Gate-C production dependencies
- **tier_target**: production-grade trajectory per `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate C: Storage, PackageFS & Content)

## Active constraints (TASK-0032 prep)

- Kernel remains untouched in `TASK-0032` scope.
- No fake-success markers: `packagefsd: v2 mounted (pkgimg)` only after real validation and index load.
- Bounded parsing/read behavior: explicit caps for image/index/entry/path/offsets.
- Security reject discipline: named `test_reject_*` paths for malformed/corrupt/traversal/OOB inputs.
- Behavior-first proof rule applies: tests must prove Soll-Verhalten, not code-shape trivia.
- Production-grade closure remains split: task-local Gate-C obligations in `TASK-0032`, VMO splice in `TASK-0033`, kernel truth in `TASK-0286/0287/0290`.

## Contract links (active)

- `tasks/TASK-0032-packagefs-v2-ro-image-index-fastpath.md`
- `tasks/TASK-0033-packagefs-v2b-vmo-splice-from-image.md`
- `tasks/TASK-0286-kernel-memory-accounting-v1-rss-pressure-snapshots.md`
- `tasks/TASK-0287-kernel-memory-pressure-v1-hard-limits-oom-handoff.md`
- `tasks/TASK-0290-kernel-zero-copy-closure-v1b-vmo-seals-reuse-truth.md`
- `docs/architecture/12-storage-vfs-packagefs.md`
- `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
- `docs/standards/RUST_STANDARDS.md`
- `docs/standards/SECURITY_STANDARDS.md`

## Carry-over note

- `TASK-0023B` external CI replay artifact closure remains an independent environmental follow-up and does not block `TASK-0031` kickoff.
