---
schema: 1
title: nxfs
role: guide
subsystem: storage
sources: [userspace/nxfs, source/services/nxfsd, source/services/vfsd/src/write_vmo_os.rs, source/services/vfsd/src/splice_os.rs, tools/fsck-nxfs]
contracts: [RFC-0071, RFC-0072, RFC-0073, ADR-0043, ADR-0044]
verified: 2026-11-05
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# nxfs

> **Purpose** — what the user-data filesystem runs today, where it sits, and which parts of RFC-0071 the code does not have.
> **Owns** — `userspace/nxfs/`, `source/services/nxfsd/`, `tools/fsck-nxfs/`
> **Contracts** — [RFC-0071](../rfcs/RFC-0071-nxfs-user-data-filesystem-contract.md) (format, transactions, classes), [RFC-0072](../rfcs/RFC-0072-vfs-v2-writable-providers-readdir-stable-errors.md) (the VFS surface it serves), [RFC-0073](../rfcs/RFC-0073-app-files-surface-svc-files-permission-filemanager-role.md) (`svc.files` + filemanager role), [ADR-0043](../adr/0043-user-data-in-dedicated-cow-fs-statefs-stays-service-kv.md) (statefs/nxfs split), [ADR-0044](../adr/0044-single-blk-device-gpt-partitions-block-layer.md) (GPT, one block owner)
> **Invariants** — Never reformat silently: an unmountable container answers `EIO`. New content lands only in fresh blocks; the journaled extent swap is the commit point. Memory per op is bounded by the caller's buffer, never by the file size.
> **Read next** — [README.md](README.md), [vmo.md](vmo.md), [statefs.md](statefs.md)

## What runs

| piece | where | what it does |
|---|---|---|
| engine | `userspace/nxfs/` | no_std, host-first: format v1 (`NXFS_VERSION = 1`), metadata journal with committed-only replay, dual checkpoint slots, crc32c on every metadata structure, canonical byte-ordered listings |
| file IO | `userspace/nxfs/src/fileio.rs`, `stream.rs` | windowed reads and windowed copy-on-write writes over the extent list; one reused working buffer per mount |
| store | `source/services/nxfsd/` (`DataStore`) | a library, not a process: `vfsd` hosts it in-process as its home provider |
| block access | `source/services/nxfsd/src/route_os.rs` | attaches the GPT `data` partition (`PART_DATA`) from `blkd` over `blockproto`; slots wired by init at spawn |
| offline check | `tools/fsck-nxfs/` | `fsck-nxfs [--repair] <container-image>` |

Limits: `MAX_FILE_BYTES` = 64 MiB, `MAX_NAME_LEN` = 255, `MAX_DEPTH` = 32.

## Where it sits

- One disk, one owner: nxfs shares the GPT device with statefs and the system volume; `blkd` owns
  the queue (ADR-0044, ADR-0067). There is no second block device.
- `vfsd` routes every path that is not `pkg:` to the store. The container root is the user home
  (`/`); first format seeds `/Bilder`, `/Videos`, `/Audio`, `/Dokumente`, `/Downloads`,
  `/Papierkorb`. RFC-0071 calls the mount `/data`; the wire carries home-absolute paths.
- Apps reach it only through `svc.files`, granted by `nexus.permission.FILES` (filemanager bundle
  type only).
- One authority per store: statefs stays the boot-critical KV (ADR-0043).

## Ops

Served by `DataStore::handle` (opcodes in `userspace/vfs-types/src/fileops.rs`):
`READDIR`, `STAT`, `READ`, `CREATE`, `MKDIR`, `WRITE_TEXT` (≤ 4096 B inline), `RENAME`, `REMOVE`,
`COPY`, plus the VMO plane in vfsd:

| path | op | how |
|---|---|---|
| read > 4096 B | `OP_READ_VMO` | vfsd streams 64 KiB windows into the caller's VMO (`splice_os.rs`); marker `vfsd: vmo splice stream ok (bytes=<n>, fallbacks=<m>)` |
| whole-file write | `OP_ARM_VMO` + `OP_WRITE_VMO` | nxfs pulls each chunk out of the VMO (`vmo_read`) into its working buffer inside one transaction (`write_vmo_os.rs`, engine `write_from`) |

First write-path consumer: screencapd (`source/services/screencapd/src/save_os.rs`).

## Not in the code

The RFC-0071 parts the engine does not implement:

- standalone `nxfsd` process and route (the store is in vfsd);
- format v2 fields, volume table, group commit, caching;
- CoW metadata tree, snapshots, clones, data checksums (Phase 3);
- encryption classes (Phase 4);
- per-subject quotas (statefs has them; see [quotas.md](quotas.md)).

Planned in TASK-0316 … TASK-0320; status lives in the ledger.

## Honesty notes

- No sealed key storage on QEMU: the planned "Device" encryption class would protect against
  medium-only theft, nothing stronger.
- Durability is proven only with `NEXUS_KEEP_BLK=1` (the launcher keeps `build/nexus.img`); the
  default launch rewrites the disk every boot.

## Proof

- Host: `cargo test -p nxfs` (unit + `userspace/nxfs/tests/crash_injection.rs` + `userspace/nxfs/tests/write_from.rs`).
- QEMU: the write path rides the screencapd lane (`screencapd: saved (kind=`); mount lines
  `nxfsd: mounted home (rw, clean|recovered)` are UART diagnostics, not ladder markers.
