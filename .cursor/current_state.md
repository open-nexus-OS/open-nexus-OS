# Cursor Current State (SSOT)

## Current architecture state

- **last_decision (2026-04-23)**: `TASK-0032` closure is synchronized to implementation reality.
  - `tasks/TASK-0032-packagefs-v2-ro-image-index-fastpath.md` is now `Done`.
  - `docs/rfcs/RFC-0041-packagefs-v2-ro-image-index-fastpath-host-first-os-gated.md` is now `Complete`.
  - Host + OS proofs are recorded (including required pkgimg marker ladder).
- **preserved boundary**: Gate-C follow-up split remains explicit and unchanged:
  - `TASK-0033` owns VMO splice/zero-copy data-path.
  - `TASK-0286`/`TASK-0287`/`TASK-0290` own kernel production closure truths.

## Active focus (execution)

- **active_task**: `tasks/TASK-0033-packagefs-v2b-vmo-splice-from-image.md` — `Draft`
- **recently_closed**: `tasks/TASK-0032-packagefs-v2-ro-image-index-fastpath.md` — `Done`
- **tier_target**: production-grade trajectory per `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate C: Storage, PackageFS & Content)

## Active constraints (for TASK-0033 kickoff)

- Kernel scope remains untouched unless explicitly owned by follow-up kernel tasks.
- No fake-success markers; success only after real behavior/proofs.
- Keep deterministic bounded read/data-plane semantics and explicit reject taxonomy.
- Do not absorb `TASK-0286`/`TASK-0287`/`TASK-0290` into packagefs service-only work.

## Contract links (active)

- `tasks/TASK-0033-packagefs-v2b-vmo-splice-from-image.md`
- `tasks/TASK-0286-kernel-memory-accounting-v1-rss-pressure-snapshots.md`
- `tasks/TASK-0287-kernel-memory-pressure-v1-hard-limits-oom-handoff.md`
- `tasks/TASK-0290-kernel-zero-copy-closure-v1b-vmo-seals-reuse-truth.md`
- `docs/architecture/12-storage-vfs-packagefs.md`
- `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`

## Carry-over note

- `TASK-0023B` external CI replay artifact closure remains independent and non-blocking for Gate-C packagefs execution.
