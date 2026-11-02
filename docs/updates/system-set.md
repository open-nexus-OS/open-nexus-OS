---
schema: 1
title: "System set v1 (.nxs v1)"
role: reference
subsystem: updates
sources: [tools/nxs-pack, userspace/updates/src/system_set.rs, tools/nexus-idl/schemas/system-set.capnp]
contracts: [RFC-0012, RFC-0089]
verified: 2026-11-02
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# System set v1 (.nxs v1)

> **Purpose** — the layout of the v1 `.nxs` system set (tar + `system.nxsindex`), who still writes and reads it, and how it differs from the v2 container the device stages.
> **Owns** — `tools/nxs-pack/`, `userspace/updates/src/system_set.rs`, `tools/nexus-idl/schemas/system-set.capnp`
> **Contracts** — [RFC-0012](../rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md) (v1 system set), [RFC-0089](../rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md) (`.nxs` v2, the format `updated` consumes)
> **Invariants** — trust comes from the anchor list passed to the parser, never from the `publisher` field inside the archive. Bounds are checked before allocation. There is no trust-free parse.
> **Read next** — [README.md](README.md), [../packaging/nxb.md](../packaging/nxb.md)

## Where v1 lives now

| piece | v1 (`system.nxsindex`) | v2 (`manifest.nxo`) |
|---|---|---|
| writer | `tools/nxs-pack` | `nx image ota`, `nx image fixtures` |
| parser | `updates::SystemSet` (`userspace/updates/src/system_set.rs`) | `updates::component_set` |
| consumer | host tests only (`tests/updates_host/tests/ota_flow.rs`) | `updated` on the device |
| schema | `SystemSetIndex` (`schemaVersion = 1`) | `ComponentManifest` (`schemaVersion = 2`) |

Both schemas live in `tools/nexus-idl/schemas/system-set.capnp`. `updated` does not read `system.nxsindex`; it stages only v2 containers from `/updates/*.nxs`. The `SignatureVerifier` trait in `system_set.rs` is shared by both parsers.

## Layout

A v1 `.nxs` is a tar archive:

```text
system.nxsindex          # entry 1
system.sig.ed25519       # entry 2
<bundle-name>.nxb/       # bundles in lexicographic byte order of the name
  manifest.nxb           # first inside each bundle
  payload.elf            # second
```

## `system.nxsindex`

Cap'n Proto binary, the single source of metadata and bundle digests.

| field | type | note |
|---|---|---|
| `schemaVersion` | UInt8 | `1` |
| `systemVersion` | Text | SemVer |
| `publisher` | Data (32) | Ed25519 public key; selects the anchor key, never establishes trust |
| `timestampUnixMs` | UInt64 | metadata only; never in markers |
| `bundles[].name` | Text | |
| `bundles[].version` | Text | SemVer |
| `bundles[].manifestSha256` | Data (32) | over `manifest.nxb` |
| `bundles[].payloadSha256` | Data (32) | over `payload.elf` |
| `bundles[].payloadSize` | UInt64 | |

## Signature

- `system.sig.ed25519` is a detached Ed25519 signature over the raw `system.nxsindex` bytes; exactly 64 bytes.
- `SystemSet::parse(bytes, verifier, trust)` checks `publisher ∈ trust` first, then the signature. Devices pass `updates::trust::BAKED_PUBLISHERS`; host tests inject their own list.

## Bounds

| constant | value |
|---|---|
| `MAX_NXS_ARCHIVE_BYTES` | 100 MiB |
| `MAX_SYSTEM_NXSINDEX_BYTES` | 1 MiB |
| `MAX_MANIFEST_NXB_BYTES` | 256 KiB |
| `MAX_PAYLOAD_ELF_BYTES` | 50 MiB per bundle |
| `MAX_BUNDLES_PER_SET` | 256 |

Tar paths are rejected when absolute, containing `..`, or containing NUL.

## Authoring (`nxs-pack`)

```bash
nxs-pack --input bundles/ --meta system-set.toml --key keys/ed25519.hex --output system-v1.0.0.nxs
```

```toml
system_version = "1.0.0"
timestamp_unix_ms = 0
```

`--key` is a file holding a 32-byte Ed25519 seed as hex; `publisher` is derived from its public key.
