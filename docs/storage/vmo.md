---
schema: 1
title: VMOs
role: guide
subsystem: storage
sources: [source/libs/nexus-abi/src/syscall/memory.rs, source/libs/nexus-wire/src/payload_vmo.rs, userspace/nexus-ipc/src/armed_vmo.rs, userspace/memory]
contracts: [RFC-0100, RFC-0097, RFC-0040, ADR-0077, ADR-0080, ADR-0079]
verified: 2026-11-03
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# VMOs

> **Purpose** — what a VMO is in this system, which calls make and move one, and the one header and arming pattern every bulk-byte service uses.
> **Owns** — `source/libs/nexus-wire/src/payload_vmo.rs`, `userspace/nexus-ipc/src/armed_vmo.rs`, `userspace/memory/`
> **Contracts** — [RFC-0100](../rfcs/RFC-0100-memory-object-model-v2-commit-on-touch-unified-gpu-memory-pressure.md) (memory object model v2), [RFC-0097](../rfcs/RFC-0097-payload-vmo-header-v2-pkg-passthrough.md) (one payload-VMO header), [RFC-0040](../rfcs/RFC-0040-zero-copy-vmos-v1-plumbing-host-first-os-gated.md) (v1 plumbing), [ADR-0077](../adr/0077-memory-is-committed-on-first-touch-the-kernel-touches-user-memory-only-through-a-checked-range.md) (commit on touch), [ADR-0080](../adr/0080-a-process-runs-from-a-sealed-image-text-mapped-from-its-frames-data-copied-no-other-exec-path.md) (sealed images), [ADR-0079](../adr/0079-user-cache-maintenance-is-the-right-of-a-non-coherent-devices-capability.md) (cache maintenance)
> **Invariants** — Bulk bytes never ride an IPC frame; they go into a VMO and are released by a header written last, in one 16-byte write. A reused VMO gets `ZEROED_HEADER` before it is armed again. A served VMO is keyed by the kernel sender id, never by a payload field.
> **Read next** — [README.md](README.md), [nxfs.md](nxfs.md), [../packaging/nxb.md](../packaging/nxb.md)

## What a VMO is

A kernel memory object addressed by a capability (RFC-0100). It dies with its last capability.
Pages are committed on first touch (ADR-0077): a reserved range costs nothing until a mapping,
a `vmo_write` or the kernel touches it.

## Calls (`source/libs/nexus-abi/src/syscall/`)

| call | does |
|---|---|
| `vmo_create`, `vmo_create_lazy` | plain object; lazy = committed on touch |
| `vmo_create_for`, `vmo_create_for_lazy`, `vmo_create_contiguous` | object inside a device's DMA reach (needs the device capability) |
| `vmo_write`, `vmo_read` | kernel copy in/out — no mapping needed |
| `vm_map`, `vm_unmap` | map into the caller's own space only (ADR-0091) |
| `vmo_share_readonly` | read-only alias: holders cannot write or map writable (glyph atlas, `source/services/execd/src/atlas_vmo.rs`) |
| `vmo_seal` | frozen for the rest of its life; what `exec_vmo` runs a process from (ADR-0080) |
| `sealed_image` | write bytes in bounded chunks, then seal; keep the handle for later spawns of the same image |
| `vmo_runs`, `vmo_dma_base` (`dma.rs`) | bus addresses of a range for a device |
| `vmo_clean` | cache maintenance for a non-coherent device (ADR-0079); no-op on coherent QEMU devices |
| `vmo_unlock_purgeable`, `vmo_lock_purgeable` (`pressure.rs`) | let the kernel drop a lazy object's pages under pressure; lock reports whether a purge happened |
| `vmo_destroy` | refused while another capability still references the range |

## The payload-VMO header (RFC-0097)

`nexus_wire::payload_vmo` is the only definition:

```
magic "NXVR" (4) | status u16 LE | rsv (2) | len u32 LE | rsv (4)    -- 16 bytes; data at offset 16
```

- `status` is a code from the one status table (`source/libs/nexus-wire/src/status.rs`); `0` = OK.
- The writer puts the payload down first and the header last. A fresh VMO is all-zero, so a
  visible magic means complete data.
- The consumer waits for the reply message (or the server's death), never polls the header.

## Arming: one capability per message

A message moves exactly one capability. A request that wants an answer moves its reply cap, so a
VMO op is two messages:

1. `OP_ARM_VMO` moves the VMO. The server stores it in `nexus_ipc::armed_vmo::ArmedVmos`, keyed
   by kernel sender id (8 rows; a second ARM replaces and releases the first).
2. The VMO op moves the reply cap and consumes the armed VMO.

| server | `OP_ARM_VMO` | consumed by |
|---|---|---|
| bundlemgrd | 12 (`source/libs/nexus-wire/src/bundlemgrd.rs`) | `GET_PAYLOAD`, `GET_BUNDLE_ELF`, `GET_INDEX`, `GET_FILE_VMO` |
| vfsd | 15 (`userspace/vfs-types/src/fileops.rs`) | `OP_WRITE_VMO` (whole-file write into nxfs) |
| blkd | 5 (`userspace/storage/src/blockproto.rs`) | `OP_READ_VMO` (partition range → VMO, ≤ 16 MiB), `OP_RELEASE_VMO` |

## Consumers

- **App payloads:** execd arms a lazy VMO of `payload_vmo::VMO_LEN` (header + `MAX_LEN`, 16 MiB;
  ADR-0093) at bundlemgrd and asks `GET_PAYLOAD`; bundlemgrd fills it (payload first, header
  last) and only the bundle's pages are committed; execd hands it to the child, which holds the
  header's length against `MAX_LEN` before it reads a byte.
- **`pkg:/` reads > 4096 B:** the caller's VMO is forwarded vfsd → packagefsd → bundlemgrd; only
  bundlemgrd writes, after the entry hashes to its index digest.
- **Home (nxfs) reads > 4096 B:** vfsd streams 64 KiB windows into the caller's VMO.
- **Home writes:** `OP_ARM_VMO` + `OP_WRITE_VMO` at vfsd; nxfs pulls the bytes with `vmo_read`
  inside one transaction. First consumer: screencapd.
- **Processes:** `exec_vmo` runs a sealed image; text is mapped from the image's frames.
- **GPU surfaces:** gpud makes a lazy object per app surface, bound read-only into windowd's GPU
  context (RFC-0114); the app `vmo_clean`s what it drew before presenting.

## Host crate `nexus-vmo` (`userspace/memory/`)

The RFC-0040 host-first typed API: `Vmo::create`, `from_bytes`, `from_file_range` (host fixture),
bounded `write`, `map_ro`, `slice`, deny-by-default `transfer_to` / `transfer_to_slot`, plus
counters (`copy_fallback_count`, `control_plane_bytes`, `bulk_bytes`, `map_reuse_hits/misses`).
Services do not use it; they call `nexus-abi` directly.

```bash
cargo test -p nexus-vmo -- --nocapture
cargo test -p nexus-vmo -- reject --nocapture
```

The old two-process OS proof (`SELFTEST: vmo share ok`) is retired. Seal and purge work that
TASK-0290 named moved to TASK-0333.

## Proof

`SELFTEST: vfs splice roundtrip ok`, `SELFTEST: pkgimg vmo ok (bytes=0x…)`,
`SELFTEST: pkgimg vmo oversize deny ok`, `SELFTEST: vfs inline oversize deny ok`
(`source/apps/selftest-client/proof-manifest/markers/vfs.toml`); header gate `just payload-vmo`.
