---
title: TASK-0293 nxfsd OS bring-up: writable /data mount + stash writes (v1: 2nd blk device, vfsd-hosted DataStore) + NEXUS_KEEP_BLK
status: In Review
owner: @runtime
created: 2026-07-15
depends-on:
  - TASK-0291
  - TASK-0292
follow-up-tasks:
  - TASK-0295
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Contract seed (this task, store): docs/rfcs/RFC-0071-nxfs-user-data-filesystem-contract.md
  - Contract seed (this task, VFS writes): docs/rfcs/RFC-0072-vfs-v2-writable-providers-readdir-stable-errors.md
  - Block topology decision: docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md
  - Track: tasks/TRACK-STASH-USER-DATA-FS.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

nxfs exists as a proven host crate (TASK-0292) but nothing serves it in the OS. ADR-0044 fixes the
topology: **one** virtio-blk device with a GPT (`state` + `data` partitions); `virtioblkd` is
promoted from proof-stub to the single device owner (one virtqueue = one owner) serving
partition-scoped block IO; statefsd and the new nxfsd become `RemoteBlockDevice` clients. The
launcher currently wipes `build/blk.img` every boot, so all existing "persist ok" markers only
prove soft-reboot replay — `NEXUS_KEEP_BLK=1` closes that honesty gap.

## Goal

Boot-proven:

1. `nexus-block` layer in `userspace/storage`: bounded CRC-validated read-only GPT parser +
   `PartitionView`/`RemoteBlockDevice` over the existing `BlockDevice` trait.
2. `virtioblkd` promoted: owns the device + virtqueue, parses GPT once, serves partition-scoped
   block IO (statefsd keeps its direct-MMIO path only until the switch is marker-proven, then it
   is deleted).
3. `nxfsd` service: mounts the `data` partition via the nxfs engine, registers as **writable
   provider** at `/data` in vfsd (RFC-0072 Phase 2 ops: Create/Write/Truncate/Mkdir/Rename/Remove).
4. Launcher: GPT image preparation + `NEXUS_KEEP_BLK=1` keep mode.
5. `svc.files` write surface (RFC-0073 Phase 2) live; stash can mkdir/rename/delete/write under
   `/data` with real UI flows.

## Non-Goals

- CoW/snapshots (RFC-0071 P3), encryption (P4), zero-copy bulk IO (TASK-0295 — inline ≤ 4 KiB
  writes are acceptable here).
- Removable/external media (TRACK-REMOVABLE-STORAGE).
- statefs journal format changes (RFC-0018 bytes stay identical on the new partition base).

## Constraints / invariants (hard requirements)

- ADR-0044 fail-closed rules: invalid GPT → `nexus-block: gpt invalid (fail)`, services stay down;
  no silent whole-device grab; no silent reformat (blank-signature explicit format only).
- statefs consumers (keystored/updated/settingsd) stay green through the ownership move — their
  contract tests + boot markers are part of THIS task's gate.
- Default boot behavior unchanged (wipe + deterministic markers); keep-blk is opt-in.
- Driver IRQ/timer discipline per existing traps: notify endpoints on idle control-reply slots,
  never server recv slots.
- No `unwrap/expect`; bounded queues for the block IPC plane.

## Red flags / decision points

- **RED (boot-critical move)**: statefsd switching from direct MMIO to virtioblkd IPC is the
  riskiest step — stage it: (1) virtioblkd serves `data` only, statefsd untouched; (2) statefsd
  switches behind a feature gate + markers; (3) direct path deleted.
- **YELLOW (block IPC plane)**: 8 KiB frames vs 512-byte sectors — batch sector ranges per
  request; move to VMO bulk once TASK-0295 lands (contract allows both from day one).
- **YELLOW (/data layout)**: top-level layout (shared tree vs per-app homes) — decide here with
  RFC-0073's namespace policy hook; record the decision in the RFC's open-questions resolution.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- `cargo test -p nexus-storage` (or crate name): GPT parser negatives (bad CRC, overlapping
  partitions, truncated entries → fail-closed), PartitionView bounds (no out-of-partition IO),
  RemoteBlockDevice contract test against a mock server.
- statefs engine tests re-run over a PartitionView (same journal bytes, nonzero base).

### Proof (OS / QEMU) — required

- `virtioblkd: serving (parts=state,data)`
- `nxfsd: mounted /data (rw, gen=<n>)`
- `vfsd: rw mount ok (/data)`
- `SELFTEST: nxfs txn atomic ok`, `SELFTEST: vfs write denied on ro ok` (packages stays RO)
- `stash: write ok` (create + rename + delete through the UI path)
- Cold boot: two-boot harness with `NEXUS_KEEP_BLK=1` → `nxfs: persisted across cold boot`
- statefs regression: existing persist selftests + keystored/updated markers stay green.

## Touched paths (allowlist)

- `userspace/storage/` (nexus-block: GPT + PartitionView + RemoteBlockDevice)
- `source/services/virtioblkd/` (promotion), `source/drivers/storage/virtio-blk/` (only if flush
  hooks are missing)
- `source/services/nxfsd/` (new service shell over `userspace/nxfs`)
- `source/services/statefsd/` (backend switch, staged), `source/services/vfsd/` (RW provider),
  `userspace/nexus-vfs/`
- app-host runtime + `userspace/apps/stash/` (write flows)
- `scripts/qemu-launcher.sh` (GPT prep + NEXUS_KEEP_BLK), `scripts/qemu-test.sh`
- init wiring/manifests for the new service (behind the execd probe block, per ctrl-plane trap)
- `docs/storage/nxfs.md`, `docs/storage/statefs.md`, `docs/architecture/12-storage-vfs-packagefs.md`

## Plan (small PRs)

1. nexus-block (host) + launcher GPT prep + keep-blk.
2. virtioblkd promotion serving `data`; nxfsd + `/data` RO mount first (read path proven).
3. RFC-0072 write ops in vfsd + nxfsd writable registration + svc.files writes + stash flows.
4. statefsd staged switch to RemoteBlockDevice; delete direct path after markers hold.
5. Cold-boot two-boot harness + full marker ladder.

## Progress snapshot (2026-07-15) — v1 write path boot-proven; cold-boot remount follow-up

**v1 staging decision (ADR-0043/0044 amended):** to deliver "stash writes + persists" without
destabilizing the boot-critical statefs path, v1 ships a **second virtio-blk device** (`data.img`)
driven by the proven `VirtioBlkMmio`, with the nxfs `/data` provider (`nxfsd::DataStore`) hosted
**in-process by vfsd** — no new init service, only a 2nd MMIO grant + launcher device. The GPT
parser + `PartitionView` + partition-scoped block IPC codec (`userspace/storage::{gpt,blockproto}`,
host-tested) are the substrate for the deferred single-device consolidation.

- [x] `userspace/storage::gpt` (bounded RO GPT parser + `PartitionView`) + `::blockproto`
  (partition-scoped block IPC codec) — host-tested (12 storage tests green), consolidation substrate.
- [x] `source/services/nxfsd` = `DataStore` library: owns the data device, `Nxfs::open_or_format`
  (mounts existing / mkfs blank, device consumed once), serves list/stat/read + mkdir/create/
  writeText/rename/remove; `nexus-vfs-types::fileops` shared op codec.
- [x] vfsd os_lite routes `/data` frames to the in-process `DataStore` (verbatim forward; store
  strips the mount prefix), honest EIO until mounted.
- [x] init: probe finds two blk slots, grants device 2 to vfsd (cap slot 49); `policies/base.toml`
  gives vfsd `device.mmio.blk`. Launcher: 2nd `-drive`/`-device` + `NEXUS_KEEP_BLK=1`.
- [x] `svc.files.mkdir/remove` + app-host write arm; stash opens `/data`, "New folder" → real mkdir.
- [x] **Write path boot-proven (fresh, visible virgl boot):** `nxfsd: mounted /data (rw, clean)` →
  `files.list ok (n=0)` (empty) → click New → `files.mkdir ok` → reload `files.list ok (n=1)` →
  **"New Folder" visible in the stash listing** (screenshot). Full write path on the real container.
- [x] **Cold-boot persistence boot-proven (`NEXUS_KEEP_BLK=1`):** create a folder → reboot keeping
  the images → `nxfsd: mounted /data (rw, clean)` → `files.list ok (n=1)` → **"New Folder" still
  visible** after the reboot (screenshot). The nxfs container survives a cold boot.

### The cold-boot remount deadlock (found + fixed during bring-up)

The first cold-boot attempts HUNG: `Nxfs::mount` read the FULL journal region (64 blocks =
512 sequential sector reads) and the virtio-blk driver deadlocks on a long sequential read run on
the 2nd device (statefs on device 1 does ~64 reads and is fine; the driver's `nsec()` timeout also
did not fire, so no `virtio-blk: timeout`). Pinned with a feature-gated mount tracer
(`nxfs/trace`): the hang was precisely the journal read. **Fix: `Nxfs::mount` reads the journal
INCREMENTALLY and stops at the first all-zero (unused) block** — the used journal is a couple of
blocks, so mount now does ~32 sector reads (well under statefs's working count). This both avoids
the driver deadlock and makes mount fast; the underlying virtio-blk long-read-run bug is a
separate driver hardening follow-up (not on this milestone's path). Host crash-injection suite
re-run green with the incremental read.
