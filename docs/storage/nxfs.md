# nxfs — the user-data filesystem (`/data`)

CONTEXT: One-page orientation for nxfs. The authoritative contract is RFC-0071; execution truth
lives in the track and its tasks. This page only says what it is, where it stands, and where the
details live.
OWNERS: @runtime
STATUS: Contract seeded (2026-07-15); no code yet — do not cite nxfs as existing behavior.

## What

The dedicated user-data filesystem service: `nxfsd` serving a GPT `data` partition, mounted
read-write at `/data` through vfsd. Designed production-grade from the contract up
(APFS-inspired): container/volume model, crash-atomic transactions (2PC journal + dual checkpoint
slots), crc32c metadata integrity, CoW/snapshots/clones (Phase 3), per-class AEAD encryption keyed
via keystored+HKDF (Phase 4), VMO zero-copy bulk IO.

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
- Until `NEXUS_KEEP_BLK=1` harnesses run, no cold-boot durability claims anywhere in this stack.
