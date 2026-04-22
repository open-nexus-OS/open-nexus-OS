<!-- Copyright 2024 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Bundle Manager Manifest Schema

**Status**: Updated 2026-04-22 (v1.2 supply-chain fields)  
**Canonical source**: ADR-0020, `tools/nexus-idl/schemas/manifest.capnp`

**Scope:** this page documents the unified manifest format used across the repo.  

For the OS daemon, see `docs/architecture/15-bundlemgrd.md`.

## Unified Format (v1.0+)

As of TASK-0007 v1.0, the repository uses a **single source of truth** for bundle manifests:

- **On-disk format**: `manifest.nxb` (Cap'n Proto binary)
- **Human-editable source**: `manifest.toml` (TOML)
- **Tooling**: `nxb-pack` compiles TOML → binary
- **Parsing**: `bundlemgr` (host) + `bundlemgrd` (OS) use Cap'n Proto parser

**Rationale**: Resolves 3-way format drift (JSON/TOML/nxb). See ADR-0020 for full decision.

## Canonical OS packaging contract

`.nxb` bundles use a deterministic directory layout:

```text
bundle.nxb/
├── manifest.nxb          (Cap'n Proto binary; canonical contract)
├── payload.elf           (ELF64/RISC-V)
└── meta/
    ├── sbom.json         (CycloneDX JSON 1.5; interop artifact)
    └── repro.env.json    (schema-versioned JSON repro metadata)
```

**Do not** treat TOML ordering/whitespace as a stable on-disk contract. TOML is an **input format** only.

## Required fields (BundleManifest)

| Field | Type | Description |
|------------|-----------------|----------------------------------------------|
| `name` | `Text` | Unique bundle identifier (non-empty). |
| `semver` | `Text` | Human-readable bundle version (SemVer). |
| `abilities` | `List(Text)` | Declared abilities provided by the bundle. |
| `capabilities` | `List(Text)` | Capabilities required by the bundle. |
| `minSdk` | `Text` | Minimum supported NEURON SDK version. |
| `publisher` | `Data` | 16 bytes (canonical TOML form: 32 lowercase hex chars). |
| `signature` | `Data` | Detached Ed25519 signature (64 bytes). |
| `payloadDigest` | `Data` | SHA-256 of `payload.elf` (32 bytes). |
| `payloadSize` | `UInt64` | Size of `payload.elf` in bytes. |
| `sbomDigest` | `Data` | SHA-256 of `meta/sbom.json` (32 bytes). |
| `reproDigest` | `Data` | SHA-256 of `meta/repro.env.json` (32 bytes). |

Validation is strict at the manifest parser boundary:

- core textual fields must be valid and non-empty,
- `publisher` must be 16 bytes,
- `signature` must be 64 bytes,
- digest fields must be either empty (legacy input path) or exactly 32 bytes.

## Errors

The parser returns `bundlemgr::Error` with the following variants:

- `Decode(String)` – Cap'n Proto decode failure (malformed `manifest.nxb`).
- `MissingField(&'static str)` – a required field was not present (schema evolution).
- `InvalidField { field, reason }` – a field could not be interpreted (for example, semver parse failure).

This structured error model allows callers to present precise feedback while
failing closed on malformed contract bytes.

## Notes on drift

`docs/bundle-format.md` documents a legacy tar-based bundle concept and is explicitly marked as drifted. For current OS work, follow `docs/packaging/nxb.md`, `RFC-0039`, and the owning task stop conditions/proofs.
