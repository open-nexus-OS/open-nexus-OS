---
title: TASK-0292 nxfs v1 core (host-first): on-disk format P1 + transactions + checksums + replay + fsck-nxfs + crash-injection determinism
status: Draft
owner: @runtime
created: 2026-07-15
depends-on:
  - TASK-0291
follow-up-tasks:
  - TASK-0293
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Contract seed (this task): docs/rfcs/RFC-0071-nxfs-user-data-filesystem-contract.md
  - Architecture split: docs/adr/0043-user-data-in-dedicated-cow-fs-statefs-stays-service-kv.md
  - Track: tasks/TRACK-STASH-USER-DATA-FS.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

RFC-0071 fixes the nxfs contract (superblock with dual checkpoint slots, object table, extents,
bounded 2PC metadata journal, crc32c metadata checksums, volume table, error mapping). This task
builds the engine as a pure host-first crate — the same discipline that made statefs v1 solid:
all format/transaction/replay logic testable against `MemBlockDevice` and image files, no OS
dependency, service shell comes later (TASK-0293).

## Goal

`userspace/nxfs` crate implementing RFC-0071 Phase 1:

- format (mkfs on a blank device), mount (checkpoint selection), full read/write op set
  (create/write/read/truncate/mkdir/readdir/rename/remove/stat) as library calls,
- transactions: every mutating op journaled `TXN_BEGIN…TXN_COMMIT`; replay applies committed-only,
- dual checkpoint slots + generation counters; checkpoint flip commit protocol,
- crc32c on all metadata; `EINTEGRITY` fail-closed,
- `tools/fsck-nxfs` host tool (validate/replay/repair, exit codes 0/1/2).

## Non-Goals

- OS service, IPC, VMO plumbing, GPT (TASK-0293).
- CoW data path, snapshots/clones, data checksums (RFC-0071 Phase 3).
- Encryption (Phase 4). Object-record fields for class/flags exist but are inert.

## Constraints / invariants (hard requirements)

- All RFC-0071 bounds enforced as constants with tests (name ≤ 255, depth ≤ 32, bounded journal,
  bounded dirty set, bounded extents per record + continuation).
- Deterministic: crash-injection tests replay identical images to identical states; no
  clock/randomness in the format path (timestamps injected by caller).
- `#![forbid(unsafe_code)]`; no `unwrap/expect` outside tests; modular files (~600 LOC per module:
  format/, journal/, tree/, alloc/, txn/, fsck lib).
- Reuse `userspace/storage` `BlockDevice` trait — no parallel block abstraction.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- `cargo test -p nxfs`:
  - mkfs + mount + roundtrip op matrix
  - crash injection at **every record boundary** of scripted op sequences → committed-only state
  - rename atomicity: exactly-one-name-visible across all cut points
  - checkpoint-flip torn-write: older valid slot mounts
  - idempotent replay; corrupt-record truncation reported not half-applied
  - every RFC-0072-mapped error code has a negative test
- `cargo test -p fsck-nxfs`: exit-code matrix, orphan repair, deterministic report.

### Proof (OS / QEMU)

None (host-only task by design). OS markers arrive with TASK-0293.

## Touched paths (allowlist)

- `userspace/nxfs/` (new crate)
- `tools/fsck-nxfs/` (new host tool)
- `userspace/storage/` (only if the BlockDevice trait needs a flush hook; no format logic here)
- `docs/storage/nxfs.md` (status flip)

## Plan (small PRs)

1. Format types + mkfs + mount/checkpoint selection + superblock tests.
2. Journal + txn engine + replay + crash-injection harness.
3. Object table + directories + op set.
4. fsck-nxfs + exit-code matrix.
