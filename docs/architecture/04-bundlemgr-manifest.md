<!-- Copyright 2024 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Bundle Manager Manifest Schema

**Status**: Updated 2026-01-15 (unified to manifest.nxb)  
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
├── manifest.nxb (Cap'n Proto binary)
└── payload.elf (ELF64/RISC-V)

```text

**Do not** treat TOML ordering/whitespace as a stable on-disk contract. TOML is an **input format** only.

## Required fields

| Field      | Type            | Description                                  |
|------------|-----------------|----------------------------------------------|
| `name`     | `string`        | Unique bundle identifier (non-empty).        |
| `version`  | `string` (semver) | Human readable bundle version.              |
| `abilities` | `array<string>` | Declared abilities provided by the bundle.   |
| `caps`     | `array<string>` | Capabilities required by the bundle.         |
| `min_sdk`  | `string` (semver) | Minimum supported NEURON SDK version.       |
| `publisher` | `string` (hex)  | 32 lowercase hex chars identifying publisher |
| `sig`      | `string` (hex)  | Detached signature (64 bytes; hex)           |

Unknown keys are not fatal—the parser records a warning string for each
unexpected entry so build tooling can surface them. String fields are trimmed
and must not be empty, and arrays must contain non-empty string items.

## Errors

The parser returns `bundlemgr::Error` with the following variants:

- `Decode(String)` – Cap'n Proto decode failure (malformed `manifest.nxb`).
- `MissingField(&'static str)` – a required field was not present (schema evolution).
- `InvalidField { field, reason }` – a field could not be interpreted (for example, semver parse failure).

This structured error model allows callers to present precise feedback and
continue execution when warnings (rather than errors) occur.

## Notes on drift

`docs/bundle-format.md` documents a legacy tar-based bundle concept and is explicitly marked as drifted. For current OS work, follow `docs/packaging/nxb.md` and tasks that define stop conditions and proof.
