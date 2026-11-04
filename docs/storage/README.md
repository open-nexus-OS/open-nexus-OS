---
schema: 1
title: Storage
role: guide
subsystem: storage
sources: [source/services/vfsd, source/services/packagefsd, source/services/statefsd, source/services/nxfsd, source/services/blkd, userspace/nexus-vfs]
contracts: [ADR-0043, ADR-0044, ADR-0067, RFC-0072, RFC-0097, RFC-0042]
verified: 2026-11-03
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Storage

> **Purpose** — which service owns which store, how a file path reaches its bytes, and where each store's details live.
> **Owns** — `source/services/vfsd/`, `source/services/packagefsd/`, `source/services/statefsd/`, `source/services/nxfsd/`, `source/services/blkd/`
> **Contracts** — [ADR-0043](../adr/0043-user-data-in-dedicated-cow-fs-statefs-stays-service-kv.md) (one authority per store; statefs stays a KV), [ADR-0044](../adr/0044-single-blk-device-gpt-partitions-block-layer.md) / [ADR-0067](../adr/0067-one-block-owner-backend-selected-by-fdt.md) (one disk, one block owner), [RFC-0072](../rfcs/RFC-0072-vfs-v2-writable-providers-readdir-stable-errors.md) (VFS v2 surface, stable error codes), [RFC-0097](../rfcs/RFC-0097-payload-vmo-header-v2-pkg-passthrough.md) (the payload-VMO header), [RFC-0042](../rfcs/RFC-0042-sandboxing-v1-vfs-namespaces-capfd-manifest-permissions-host-first-os-gated.md) (namespaces + CapFd)
> **Invariants** — One authority per store; no file semantics in statefs. Only the hop that verified the bytes writes a success header into a payload VMO. Above the block owner, no service knows which hardware answered.
> **Read next** — [statefs.md](statefs.md), [nxfs.md](nxfs.md), [vmo.md](vmo.md), [quotas.md](quotas.md), [../packaging/nxb.md](../packaging/nxb.md)

## Map

| store | authority | reached through | nature | details |
|---|---|---|---|---|
| `pkg:/` (`/packages/<name>@<version>/…`) | `packagefsd`, bytes from `bundlemgrd` | `vfsd` | read-only bundle content of the verified system volume (pkgimg v3) | [../packaging/nxb.md](../packaging/nxb.md) |
| user home (RFC-0071's `/data`) | nxfs engine, hosted in `vfsd` | `vfsd` | read-write user files | [nxfs.md](nxfs.md) |
| `/state/…` | `statefsd` | its own IPC protocol (not `vfsd`) | boot-critical service-state KV records | [statefs.md](statefs.md), [quotas.md](quotas.md) |

`vfsd` routes by path: a path that starts with `pkg:` goes to `packagefsd`; every other path
belongs to the nxfs home (`is_home_path` in `source/services/vfsd/src/os_lite.rs`). The home
container's root is `/` (`/Bilder`, `/Dokumente`, … seeded on first format) — there is no
`/data` prefix on the wire.

## Below the stores: one block owner

All stores sit on one disk with one owner, `blkd`. nxboot records the boot disk in
`/chosen/nexus,boot-disk`; init grants `blkd` exactly that device; the grant's kind selects the
backend — virtio-blk on QEMU (`source/drivers/storage/virtio-blk/`), the SDHCI core on the board
and in the `ci-os-sdhci` lane (`source/drivers/storage/sdhci/`). Before it touches the controller
`blkd` asks `socd` to bring the disk's node up (RFC-0106).

`blkd` serves GPT partitions over `blockproto` (`userspace/storage/src/blockproto.rs`,
`PART_STATE`, `PART_DATA`, …) behind a deny-by-default partition gate. Clients attach with
`storage::remote_blk::RemoteBlockDevice` on fixed slots that init wires at spawn: statefsd takes
the `state` partition, the nxfs store in vfsd takes `data`, bundlemgrd reads the system volume.
Markers: `blkd: backend ok (kind=… record=… sectors=…)`, `blkd: gpt ok (parts=8)`.

## Services

### `vfsd` — the client-facing file surface

- Ops: open/read/stat/close, `ReadDir` (bounded pages), and on the home mount
  create/mkdir/writeText/rename/remove/copy. `pkg:/` rejects writes with `EROFS`.
- Every reply carries a code from the one RFC-0072 table (`source/libs/nexus-wire/src/status.rs`).
- Per-app confinement (namespaces + CapFd, RFC-0042) applies to every op
  (`source/services/vfsd/src/namespace.rs`, `sandbox.rs`).
- Hosts the nxfs `DataStore` in-process (`source/services/nxfsd/` is a library, not a process).
- VMO data plane: reads in `splice_os.rs`, whole-file writes in `write_vmo_os.rs` (see below).
- Apps reach files only through the SDK route `svc.files` (app-host `effect_files.rs`), granted by
  `nexus.permission.FILES`, which `nxb-pack` allows only for `bundle_type = "filemanager"`
  (RFC-0073).

### `packagefsd` — read-only `pkg:/`

- At boot asks `bundlemgrd` for the NXSV-verified pkgimg v3 index (`GET_INDEX`) and builds its
  registry (bundle@version, paths, sizes). Entry bytes stay on the volume.
- Marker: `packagefsd: mounted (system volume slot=<s> bundles=N files=M)`. Without a verified
  volume (recovery boots) the built-in seed registry mounts as `Legacy`.
- `/packages/<name>@<version>/…` is the versioned view; `pkg:/<name>/…` resolves to the active
  version.
- Host mode (`std_server.rs`) can mount a pkgimg v2 image from `PACKAGEFSD_PKGIMG_PATH`.

### `statefsd` — `/state` records

Not reachable through `vfsd`. Capability-gated KV protocol, journal on the `state` partition.
See [statefs.md](statefs.md).

## How bytes move

The entry size picks the path, not the caller (RFC-0072, RFC-0097):

| size | path | who writes the bytes |
|---|---|---|
| ≤ `INLINE_IO_MAX` (4096 B) | inline read, one bounded reply frame | the provider |
| > 4096 B, home | `OP_READ_VMO`: vfsd streams 64 KiB windows into the caller's VMO | vfsd (nxfs) |
| > 4096 B, `pkg:/` | `OP_READ_VMO`: the caller's VMO is cap-moved vfsd → packagefsd → bundlemgrd | bundlemgrd only, after the entry hashes to its index digest |
| write, home | `OP_ARM_VMO` then `OP_WRITE_VMO`: nxfs pulls the content out of the VMO in one transaction | vfsd (nxfs) |

- An inline read above the cap is `E2BIG`, never a truncation.
- A VMO too small for the entry is refused with `TooBig` before a byte is written; packagefsd
  checks it because it knows both sizes.
- Hops that only relay may write an ERROR header, never a success header.
- A write is two messages because a message moves one capability: ARM moves the VMO
  (kept per kernel sender id in `nexus_ipc::armed_vmo`), WRITE moves the reply cap. Cap:
  `MAX_WRITE_VMO_BYTES` = 64 MiB. First consumer: screencapd saving screenshots.

Header and table details: [vmo.md](vmo.md).

## Client boundary

Clients use `userspace/nexus-vfs/`. Host tests wire `VfsClient::from_loopback`; OS builds use
kernel IPC.

## Proof

| layer | where |
|---|---|
| host E2E | `tests/vfs_e2e/`, `cargo test -p statefs`, `cargo test -p nxfs`, `tests/state_quota_host/` |
| QEMU markers | `source/apps/selftest-client/proof-manifest/markers/vfs.toml` (`SELFTEST: vfs splice roundtrip ok`, `SELFTEST: pkgimg vmo ok (bytes=0x…)`, `SELFTEST: vfs inline oversize deny ok`, readdir/sandbox/capfd markers) |
| harness | `scripts/qemu-test.sh` |

When you change path semantics, aliasing or error codes: update the host E2E tests first, then
the marker manifest only if real behavior changed.
