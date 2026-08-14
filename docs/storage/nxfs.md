# nxfs — the user-data filesystem (`/data`)

CONTEXT: One-page orientation for nxfs. The authoritative contract is RFC-0071; execution truth
lives in the track and its tasks. This page only says what it is, where it stands, and where the
details live.
OWNERS: @runtime
STATUS: v1 shipped + boot-proven (2026-07-19; updated 2026-08-14) — engine + fsck host-proven
(TASK-0292), `/data` RW mount + cold-boot persistence boot-proven (TASK-0293), VMO splice reads
(TASK-0295). Shipped in the v1 STAGING shape (see below); the end-state ladder that retires the
staging is TASK-0314–0320.

## What

The dedicated user-data filesystem service: `nxfsd` serving a GPT `data` partition, mounted
read-write at `/data` through vfsd. Designed production-grade from the contract up
(APFS-inspired): container/volume model, crash-atomic transactions (2PC journal + dual checkpoint
slots), crc32c metadata integrity, CoW/snapshots/clones (Phase 3), per-class AEAD encryption keyed
via keystored+HKDF (Phase 4), VMO zero-copy bulk IO.

**What actually runs today (v1 staging, honest):** the engine (`userspace/nxfs`: format, 2PC
journal with committed-only replay, dual checkpoint slots, crc32c fail-closed, crash-injection
suite) + `tools/fsck-nxfs`, serving `/data` on a **2nd** virtio-blk device with the store hosted
**in-process by vfsd** (`nxfsd::DataStore`) — not yet the GPT single-device topology, not yet an
own process. Known v1 gaps (owned by the ladder): whole-file rewrite on write + 4 MiB file cap +
whole-state checkpoint blob (TASK-0316/0319), FLUSH per txn / no group commit / no caching
(TASK-0316), 512 B queue-depth-1 poll-driven block driver (TASK-0314), no VMO write path
(TASK-0317), no volume table / object-record fields on disk yet (TASK-0316), no perf contract or
bench gate (TASK-0318).

It exists because statefs is (deliberately) not this: statefs stays the small boot-critical
service-state KV (ADR-0043). One authority per store: `/state` = statefsd, `/packages` =
packagefsd, `/data` = nxfsd.

## Where everything lives

| aspect | source of truth |
|---|---|
| Full contract (format, txns, classes, markers) | `docs/rfcs/RFC-0071-nxfs-user-data-filesystem-contract.md` |
| statefs/nxfs split decision | `docs/adr/0043-user-data-in-dedicated-cow-fs-statefs-stays-service-kv.md` |
| Block topology (GPT, virtioblkd owner, keep-blk) | `docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md` |
| VFS surface it mounts into (ReadDir/writes/errors) | `docs/rfcs/RFC-0072-vfs-v2-writable-providers-readdir-stable-errors.md` |
| App surface above it (`svc.files`, filemanager role) | `docs/rfcs/RFC-0073-app-files-surface-svc-files-permission-filemanager-role.md` |
| Milestone ladder + status | `tasks/TRACK-STASH-USER-DATA-FS.md` |
| Engine (host-first) | `tasks/TASK-0292-nxfs-v1-core-host-first.md` → `userspace/nxfs` |
| OS bring-up | `tasks/TASK-0293-nxfsd-os-bringup-gpt-mount-data-keepblk.md` → `source/services/nxfsd` |

## Honesty notes

- No sealed key storage on QEMU targets: the Phase 4 "Device" encryption class protects against
  medium-only theft, nothing stronger — markers and docs say exactly that.
- Cold-boot durability IS proven since TASK-0293: the `NEXUS_KEEP_BLK=1` launcher mode keeps the
  block images across boots and the `/data` persistence marker ladder runs against it. The
  default launcher still wipes images per boot — only keep-blk runs prove durability.
