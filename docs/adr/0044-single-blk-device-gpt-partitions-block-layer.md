# ADR-0044: One virtio-blk device, GPT-partitioned, with a shared partition-view block layer — plus a keep-blk mode for cold-boot persistence proofs

- Status: Accepted. Decides how statefsd and nxfsd share block storage before nxfs bring-up
  (TASK-0293) wires it, and while the switch is still free of data-migration cost.
- Created: 2026-07-15
- Builds on: ADR-0023 (statefs owns persistence v1), ADR-0043 (statefs/nxfs split),
  RFC-0017 (capability-gated MMIO), `tasks/TRACK-REMOVABLE-STORAGE.md` (future external media).
- Contract: `docs/rfcs/RFC-0071-nxfs-user-data-filesystem-contract.md` (consumer),
  `docs/rfcs/RFC-0018-statefs-journal-format-v1.md` (unchanged journal format on a new base offset).
- Execution (SSOT): `tasks/TASK-0293-nxfsd-os-bringup-gpt-mount-data-keepblk.md`

## Context

Today there is exactly one virtio-blk device; statefsd owns it whole-device via MMIO cap slot 48
and writes its journal from block 0 (`source/services/statefsd/src/os_lite.rs`,
`source/drivers/storage/virtio-blk/`). nxfs (RFC-0071) needs durable blocks too. And the launcher
recreates `build/blk.img` on every boot (`scripts/qemu-launcher.sh` `prepare_blk_image`:
`rm -f` + `truncate -s 64M`), so "persistence" is currently only ever proven across a soft reboot
(statefsd restart + replay inside one VM run).

Options considered: (a) a second virtio-blk device just for nxfs; (b) GPT-partition the one device;
(c) defer behind the `BlockDevice` trait. The user direction for this track is explicit:
production-grade and future-proof, not the minimal lab solution. Real hardware has one storage
device; "one device per filesystem" does not survive contact with a real board.

## Decision

1. **One block device, GPT-partitioned.** The disk image carries a standard GPT with (v1) two
   partitions: `state` (statefs journal) and `data` (nxfs container). GPT because it is the
   industry format real firmware/tools understand — future hardware, host inspection tooling, and
   external media (TRACK-REMOVABLE-STORAGE) all speak it.
2. **A shared partition layer in `userspace/storage`** (working name `nexus-block`): a bounded,
   read-only GPT parser (header + entries, CRC32-validated, fail-closed) and a `PartitionView`
   implementing the existing `BlockDevice` trait over `(base_lba, length)` of an underlying device.
   Both statefsd and nxfsd consume their partition through `PartitionView` — one parser, no
   per-service GPT code.
3. **statefsd becomes partition-aware in the same step** (TASK-0293). Its journal format (RFC-0018)
   is untouched — the engine already speaks `BlockDevice`, so the change is "which view it gets".
   Whole-device operation remains supported when no GPT is present (host tests, MemBlockDevice).
4. **Image preparation is host-side**: the launcher creates the GPT layout when (re)creating
   `blk.img` (deterministic tooling, e.g. `scripts/` helper); services never format partitions
   implicitly. First-boot filesystem formatting inside a partition stays each service's explicit
   blank-signature path (RFC-0071 rule: no silent reformat).
5. **One driver owner: `virtioblkd` is promoted from proof-stub to the real block-device daemon.**
   A single virtio queue cannot be safely driven by two processes, so exactly one service owns the
   MMIO window and the virtqueue. `virtioblkd` (today a stub that only maps the window) becomes the
   device owner: it parses the GPT once and serves **partition-scoped block IO** to statefsd and
   nxfsd over IPC (control ops inline; bulk sectors via VMO where it pays). Client side, a
   `RemoteBlockDevice` implements the existing `BlockDevice` trait, so the statefs/nxfs engines are
   untouched. statefsd's direct-MMIO path remains only as the no-GPT/dev fallback until the switch
   is boot-proven, then it is deleted (no permanent dual path).
6. **`NEXUS_KEEP_BLK=1`**: launcher mode that skips image recreation, enabling real cold-boot
   persistence proofs (`nxfs: persisted across cold boot`, and statefs equivalents). Default
   behavior stays wipe-per-boot so existing deterministic boots are unaffected.

## Rationale

- **Now is free**: because the image is wiped every boot, no deployed data exists; partition-aware
  statefsd costs a base offset, not a migration.
- A second QEMU device would be simpler today and wrong tomorrow: it hard-codes a topology real
  devices don't have, duplicates driver/MMIO wiring, and pushes the partition problem into the
  future where migration is no longer free.
- CRC-validated read-only GPT parsing is small, bounded, and testable host-first — consistent with
  the no-unbounded-parsing discipline; write/repartition tooling stays host-side.

## Consequences

- **Positive**: real-hardware-shaped storage stack from day one; one shared, tested partition
  seam; cold-boot persistence becomes provable (closing an honesty gap in all current
  "persist ok" markers, which only ever proved soft-reboot).
- **Cost**: TASK-0293 touches statefsd's backend selection (Mem → virtio upgrade now selects a
  PartitionView). Guarded by keeping RFC-0018 bytes identical and by host tests running the same
  engine over raw vs. partition views.
- **Risk**: a bad GPT bricks both stores' discovery → parser is fail-closed with deterministic
  markers (`nexus-block: gpt invalid (fail)`), services stay down rather than guessing; the
  launcher regenerates a valid image by default.
- **Cost (ownership move)**: statefsd's block path gains an IPC hop through virtioblkd. Accepted:
  statefs IO is small and replay-bounded; the hop buys the only safe multi-consumer topology and
  finally gives `virtioblkd` its real job. The boot-critical switch is gated on markers
  (`virtioblkd: serving (parts=state,data)`, statefs persist selftests stay green) before the
  direct-MMIO fallback is removed.
- Slot/cap layout: the device MMIO cap moves to virtioblkd; statefsd/nxfsd get IPC routes instead
  of device caps — strictly less privilege in the storage daemons (least-privilege win).
