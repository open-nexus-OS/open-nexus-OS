---
title: TASK-0316 nxfs engine v2: format v2 (contracted fields + volume table) + write-path performance (block-granular CoW, group commit, cache)
status: Draft
owner: @runtime
created: 2026-08-14
depends-on:
  - TASK-0314
follow-up-tasks:
  - TASK-0319
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Contract (this task closes the v1 gap against it): docs/rfcs/RFC-0071-nxfs-user-data-filesystem-contract.md
  - Store split: docs/adr/0043-user-data-in-dedicated-cow-fs-statefs-stays-service-kv.md
  - Ladder: tasks/TRACK-STASH-USER-DATA-FS.md
---

## Context

The shipped nxfs v1 engine (TASK-0292, Done) is far below the RFC-0071 contract it implements,
in ways that are both a **performance disaster** and a **latent format break**:

- `write()`/`truncate()` **materialize the entire file into RAM and rewrite every block**
  (`userspace/nxfs/src/fs.rs:292-346`): a 1-byte write into a 4 MiB file ≈ 16 000 serialized
  virtio requests; `OP_COPY` in the DataStore does read-all + write-all, blocking vfsd for
  seconds. `MAX_FILE_BYTES = 4 MiB` is a hard file-size cap.
- Every single-op transaction issues its own device FLUSH (`fs.rs:412`); RFC-0071 contracts
  group commit with a bounded flush interval ("5 ms vs 20 ms — measure") that was never built
  or measured.
- **Zero caching**: no block cache, no extent readahead — every read is a cold device trip.
- The object record lacks the contracted Phase-1 "inert" fields (timestamps, owner subject,
  encryption class, flags), there is **no on-disk volume table and no reserved per-extent
  checksum field** — so RFC-0071 Phase 3/4 as written would be a format break, which the
  contract explicitly forbids.
- `alloc_blocks` is a bit-by-bit O(total_blocks) scan from `data_start` on every write; the
  journal region is never zeroed after checkpoint, so once it has filled up, every mount
  re-reads and re-parses the full journal region (the TASK-0293 incremental read only helps the
  fresh case).

## Goal

**Format v2 (one deliberate, versioned step — the last planned format change before Phase 3):**

- Object record carries the contracted fields: created/modified ns, owner subject, encryption
  class (value = inherit), flags; extent continuation records lift the per-record extent bound.
- On-disk **volume table** (v1 content: the single `data` volume) + snapshot list reserved;
  snapshot ops surface returns `Unsupported` (honest, per contract).
- Reserved **per-extent data-checksum field** (written as 0 behind the superblock flag; Phase 3
  turns it on).
- Migration: mount of a v1 container upgrades via checkpoint rewrite (one-way, journaled,
  crash-safe); fsck-nxfs understands both generations during the transition.

**Write path / performance:**

- **Block-granular CoW writes**: only touched blocks get new extents; partial-extent updates;
  `MAX_FILE_BYTES` cap removed (replaced by real bounds: max extents per object via
  continuation, quota later); `OP_COPY` goes extent-wise without whole-file RAM materialization.
- **Group commit**: multi-op `run_txn`, one `TXN_COMMIT` + one FLUSH per group, bounded flush
  interval; sync vs async durability modes per the RFC ("no hidden write-back lies") — the
  interval is measured on virtio-blk and the number is written back into RFC-0071
  (closing its open question).
- **Block cache + extent readahead** (bounded, no_std bump-allocator-compatible: fixed-size
  cache arena, no per-frame alloc), serving repeated reads without device trips.
- Allocator: free-run cursor + word-wise bitmap scan (O(words), not O(bits)).
- Journal hygiene: zero (or head-record) the journal at checkpoint so mount replay stays
  incremental forever; kill the duplicated superblock read in `open_or_format`.
- Offset/append write opcodes in the store surface (the transport lands in TASK-0317; the engine
  API stops being whole-file-only here).

## Non-Goals

- CoW metadata **tree** / snapshots / clones / data-checksum enforcement — TASK-0319 (Phase 3),
  gated on its B-tree-vs-sorted-run ADR. This task only makes Phase 3 format-compatible.
- Encryption (TASK-0320 / Phase 4).
- Process extraction / IPC surface (TASK-0317).
- Bounded-resident-metadata paging (the full-RAM object map survives until the Phase-3 tree
  replaces it — documented as a known bound, not silently claimed fixed).

## Constraints / invariants (hard requirements)

- Crash-atomicity invariants of v1 are preserved and re-proven: the crash-injection suite
  (`userspace/nxfs/tests/crash_injection.rs`) runs against every new write path and the v1→v2
  migration at every record boundary.
- Fail-closed integrity unchanged: crc32c on all metadata, `EINTEGRITY` on mismatch.
- Bounded everything: cache arena size, readahead window, group-commit batch size and interval,
  extents per continuation chain.
- No fake markers: `nxfs: group commit on (interval=<n>ms)` only when batching is real.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- Existing 17 unit + crash-injection tests green on v2 paths + migration.
- New tests: partial-write touches only expected blocks (write-log assertion via SpyDevice),
  group-commit both-or-neither across restart, cache hit/miss counters, allocator cursor
  behavior, journal-zeroing keeps mount replay bounded (mount cost independent of write
  history), v1→v2 migration byte-level idempotence.
- Deterministic op-count proof: 1-byte write into a large file costs O(touched blocks), not
  O(file size) — asserted on SpyDevice write counts.

### Proof (OS / QEMU) — required

- `nxfsd: mounted /data (rw, clean)` on a v2 (and a migrated v1) container; stash write/copy
  path green; cold-boot persistence (`NEXUS_KEEP_BLK=1`) green.
- `nxfs: group commit on (interval=<n>ms)`.

## Touched paths (allowlist)

- `userspace/nxfs/`, `tools/fsck-nxfs/`
- `source/services/nxfsd/` (store surface: offset/append ops)
- `source/apps/selftest-client/`, `scripts/qemu-test.sh`
- `docs/rfcs/RFC-0071-…` (flush-interval number + format-v2 appendix; approval-gated),
  `docs/storage/nxfs.md`
