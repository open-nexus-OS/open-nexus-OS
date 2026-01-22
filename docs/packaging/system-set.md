<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Nexus System-Set Packaging (`.nxs`)

**Status**: Complete (v1.0 spec)  
**Canonical source**: `docs/rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md`

This document defines the v1.0 system-set container (`.nxs`). It is the single truth for
system-set layout and signature binding, and is designed for deterministic, signed updates.

## Layout

An `.nxs` file is a tar archive with the following layout:

```text
system.nxsindex
system.sig.ed25519
<bundle-name-1>.nxb/
  manifest.nxb
  payload.elf
<bundle-name-2>.nxb/
  manifest.nxb
  payload.elf
...
```

## Deterministic ordering

- `system.nxsindex` MUST be the first tar entry.
- `system.sig.ed25519` MUST be the second tar entry.
- Bundle directories MUST be ordered by `<bundle-name>` in lexicographic byte order.
- Within each bundle directory, entries MUST be ordered:
  1) `manifest.nxb`
  2) `payload.elf`

## System index (`system.nxsindex`)

`system.nxsindex` is a **Cap'n Proto binary** and is the single source of truth for
system-set metadata and bundle digests.

Schema location (single truth):

- `tools/nexus-idl/schemas/system-set.capnp`

Key fields:

- `schemaVersion`: UInt8 (MUST be `1` for v1.0)
- `systemVersion`: Text (SemVer)
- `publisher`: Data (32 bytes)
- `timestampUnixMs`: UInt64 (metadata; MUST NOT be used in markers)
- `bundles`: list of bundle entries:
  - `name`: Text
  - `version`: Text (SemVer)
  - `manifestSha256`: Data (32 bytes; SHA-256 over `manifest.nxb`)
  - `payloadSha256`: Data (32 bytes; SHA-256 over `payload.elf`)
  - `payloadSize`: UInt64

## Signature binding

- `system.sig.ed25519` is a detached Ed25519 signature over the raw bytes of
  `system.nxsindex`.
- The signature file MUST contain exactly 64 bytes.
- Verification MUST use `keystored.verify(pubkey, system_nxsindex_bytes, signature)`.

## Bounds (defaults)

Implementations MUST enforce size limits before allocation or extraction:

- `MAX_NXS_ARCHIVE_BYTES` (default: 100 MiB)
- `MAX_SYSTEM_NXSINDEX_BYTES` (default: 1 MiB)
- `MAX_MANIFEST_NXB_BYTES` (default: 256 KiB)
- `MAX_PAYLOAD_ELF_BYTES` (default: 50 MiB per bundle)
- `MAX_BUNDLES_PER_SET` (default: 256)

## Path safety

Tar entries MUST be rejected if any path:

- is absolute
- contains `..`
- contains NUL
- escapes the logical root

## Notes

- `.nxb` bundle layout is defined in `docs/packaging/nxb.md` and ADR-0020.
- Downgrade protection requires persistence + boot-chain anchoring and is out of scope for v1.0.

## Authoring system-sets (`nxs-pack`)

`nxs-pack` builds `.nxs` archives from an input directory of `.nxb` bundles and a small
metadata TOML file.

Example usage:

```bash
nxs-pack --input bundles/ --meta system-set.toml --key keys/ed25519.hex --output system-v1.0.0.nxs
```

Example `system-set.toml`:

```toml
system_version = "1.0.0"
timestamp_unix_ms = 0
```

Key material:

- `--key` expects a **32-byte Ed25519 seed** encoded as hex in a file.
- The publisher field in `system.nxsindex` is derived from the public key.
