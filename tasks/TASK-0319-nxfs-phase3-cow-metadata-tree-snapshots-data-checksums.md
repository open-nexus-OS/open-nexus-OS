---
title: TASK-0319 nxfs Phase 3: CoW metadata tree (ADR first) + snapshots/clones + data checksums (RFC-0071 P3)
status: Draft
owner: @runtime
created: 2026-08-14
depends-on:
  - TASK-0316
follow-up-tasks:
  - TASK-0320
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Contract (Phase 3, fixed since 2026-07-15): docs/rfcs/RFC-0071-nxfs-user-data-filesystem-contract.md
  - Prerequisite decision (to be authored HERE, step 1): the narrow "object-table: B-tree vs sorted-run/LSM" ADR RFC-0071 requires before P3
  - Ladder: tasks/TRACK-STASH-USER-DATA-FS.md (replaces milestone 6 "seed-when-ready")
---

## Context

RFC-0071 Phase 3 is contract-fixed but was parked "seed-when-ready"; per user direction
2026-08-14 the end-architecture ladder is seeded explicitly. The v1/v2 checkpoint is still **one
flat serialized blob of the entire object + directory tables** into two fixed regions, with the
full metadata resident in RAM — architecturally the same full-RAM-map shape ADR-0043 rejects
statefs for, and O(filesystem size) at mount and checkpoint. RFC-0071's own open question —
**"Phase 3 metadata tree: B-tree vs. sorted-run/LSM hybrid — decide before the Phase 3 task is
seeded (narrow ADR)"** — has no ADR yet. TASK-0316 makes the format Phase-3-compatible
(volume table, object-record fields, reserved checksum field) so this task is NOT a format break.

## Goal

- **Step 1 (gate): the narrow ADR** — B-tree vs sorted-run/LSM for the paged CoW object table,
  decided on measured v2 workloads (bench harness from TASK-0318). No tree code before the ADR
  is accepted.
- **Paged CoW metadata tree** replacing the whole-state checkpoint blob: checkpoint = CoW root
  flip over freshly allocated tree pages (the commit protocol the format reserved from day one);
  **bounded resident metadata** (page cache over tree nodes, not full maps); mount cost
  O(journal + root path), independent of filesystem size.
- **Snapshots/clones as O(1) volume-table entries** on frozen checkpoint roots; RO snapshot
  mounts through the existing vfsd surface; deletion = refcounted space reclaim (bounded,
  incremental GC).
- **Per-extent data checksums on** (the reserved field becomes live behind the superblock flag):
  verified on read, `EINTEGRITY` fail-closed; scrub pass in fsck-nxfs.

## Non-Goals

- Encryption classes (TASK-0320).
- Multi-volume UX / removable media (TRACK-REMOVABLE-STORAGE).
- statefs snapshots (TASK-0134 keeps its statefs-only slice; user-data snapshots live here, per
  its 2026-07-15 scope note).

## Constraints / invariants (hard requirements)

- Format-versioned, migration crash-safe, fsck understands old + new during transition.
- Crash-injection suite extended to tree-page boundaries: every write-prefix remount lands on
  pre- or post-state, snapshots included.
- Bounded everything: tree fanout, node size, resident-page budget, GC work per cycle.
- Snapshot semantics are honest: `Unsupported` disappears only when create/list/mount/delete all
  work with proofs; no partial claim.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- ADR accepted and linked.
- Tree unit + property tests (insert/remove/split/merge, CoW root flip, refcounts); mount-cost
  bound test (op count independent of object count beyond root path); crash-injection green
  incl. snapshot create/delete at every boundary; checksum scrub detects injected corruption
  (`test_reject_bad_extent_checksum`).

### Proof (OS / QEMU) — required

- `nxfsd: mounted /data (rw, clean)` on a P3 container; `SELFTEST: nxfs snapshot roundtrip ok`
  (create → write → mount RO snapshot → old bytes visible, live bytes changed); cold-boot green;
  storage bench (TASK-0318) budgets still met.

## Touched paths (allowlist)

- `userspace/nxfs/`, `tools/fsck-nxfs/`, `source/services/nxfsd/`, `source/services/vfsd/`
  (snapshot mount surface)
- `docs/adr/` (the new narrow ADR), `docs/rfcs/RFC-0071-…` (P3 status; approval-gated)
- `source/apps/selftest-client/`, `scripts/qemu-test.sh`, `docs/storage/nxfs.md`
