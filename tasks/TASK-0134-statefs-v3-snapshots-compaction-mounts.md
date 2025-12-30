---
title: TASK-0134 StateFS v3: named snapshots + read-only mounts + log compaction/GC (extends v2a) + tests/markers
status: Draft
owner: @runtime
created: 2025-12-25
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - StateFS v1 (KV substrate): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - StateFS v2a (2PC+compaction+fsck): tasks/TASK-0026-statefs-v2a-2pc-compaction-fsck.md
  - Write-path hardening: tasks/TASK-0025-statefs-write-path-hardening-integrity-atomic-budgets-audit.md
  - Storage error contract: tasks/TASK-0132-storage-errors-vfs-semantic-contract.md
---

## Context

The “StateFS v3” prompt combines:

- snapshots (COW views),
- compaction/GC,
- and read-only mounts for browsing snapshots.

We already have compaction work in `TASK-0026`. This task extends that direction with **named snapshots**
and a **read-only snapshot mount** concept, without claiming full POSIX filesystem semantics.

## Goal

Deliver:

1. Named snapshots:
   - `snapshot(subject, name)` creates an immutable view of the current committed state
   - `listSnaps(subject)` lists names deterministically
2. Read-only mounts:
   - `mountSnap(subject, name, mount)` exposes a read-only view (write attempts return `EROFS`)
   - mount paths and cross-subject behavior must be explicit and deterministic (no “magic global namespace”)
3. Compaction/GC:
   - bounded compaction that can reclaim unreachable data
   - `gcNow()` triggers a bounded reclaim cycle and returns `freedBytes`
   - compaction progress markers emitted periodically but bounded
4. Markers:
   - `statefs: snapshot subject=... name=...`
   - `statefs: compact progress freed=...`
5. Tests:
   - host tests for snapshot immutability + EROFS
   - compaction/GC determinism for fixtures

## Non-Goals

- Kernel changes.
- Full filesystem semantics (directories, partial writes, mmap). This remains a state substrate.
- Encryption-at-rest (separate task `TASK-0027`).

## Constraints / invariants (hard requirements)

- Deterministic snapshot naming rules and ordering.
- Bounded replay/GC work per cycle.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake markers: snapshot markers emitted only after snapshot metadata is durable (in host tests) or replayed (OS-gated).

## Stop conditions (Definition of Done)

### Proof (Host) — required

New deterministic host tests (suggested: `tests/statefs_v3_host/`):

- snapshot preserves old bytes after subsequent writes
- snapshot mount denies writes with `EROFS`
- `gcNow()` reports deterministic freed bytes for a fixed fixture workload

### Proof (OS/QEMU) — gated

Once `/state` exists in QEMU and statefsd is real:

- `SELFTEST: statefs v3 snapshot ok`
- `SELFTEST: statefs v3 compact ok`

## Touched paths (allowlist)

- `source/services/statefsd/` (extend v2a/v3 semantics)
- `userspace/statefs/` (client)
- `tests/`
- `docs/storage/statefs-v3.md` (new)

## Plan (small PRs)

1. Define snapshot metadata format and APIs (host-first)
2. Implement mount read-only view semantics
3. Implement bounded GC/compaction triggers + markers
4. Add host tests + docs + OS markers (gated)

