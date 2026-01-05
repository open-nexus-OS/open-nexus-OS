<!-- Copyright 2024 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Bundle Manager Manifest Schema

**Scope:** this page documents the host-first TOML manifest parser in `userspace/bundlemgr`.  
For the OS daemon, see `docs/architecture/15-bundlemgrd.md`.

The bundle manager crate (`userspace/bundlemgr`) includes a **host-first** manifest parser that accepts TOML.
This TOML schema is primarily used for host tooling/tests and developer ergonomics.

**Canonical OS packaging contract:** `.nxb` bundles use a deterministic directory layout with `manifest.nxb` + `payload.elf` (see `docs/packaging/nxb.md`).
Do not treat ad-hoc TOML ordering/whitespace as a stable on-disk OS contract.

## Required fields

| Field      | Type            | Description                                  |
|------------|-----------------|----------------------------------------------|
| `name`     | `string`        | Unique bundle identifier (non-empty).        |
| `version`  | `string` (semver)| Human readable bundle version.              |
| `abilities`| `array<string>` | Declared abilities provided by the bundle.   |
| `caps`     | `array<string>` | Capabilities required by the bundle.         |
| `min_sdk`  | `string` (semver)| Minimum supported NEURON SDK version.       |
| `publisher`| `string` (hex)  | 32 lowercase hex chars identifying publisher |
| `sig`      | `string`        | Detached signature (64 bytes; hex or base64) |

Unknown keys are not fatal—the parser records a warning string for each
unexpected entry so build tooling can surface them. String fields are trimmed
and must not be empty, and arrays must contain non-empty string items.

## Errors

The parser returns `bundlemgr::Error` with the following variants:

- `Toml(String)` – raw TOML parsing failure.
- `MissingField(&'static str)` – a required field was not present.
- `InvalidRoot` – the manifest root is not a TOML table.
- `InvalidField { field, reason }` – a field could not be interpreted (for
  example, a semver parse failure or an empty capability name).

This structured error model allows callers to present precise feedback and
continue execution when warnings (rather than errors) occur.

## Notes on drift

`docs/bundle-format.md` documents a legacy tar-based bundle concept and is explicitly marked as drifted. For current OS work, follow `docs/packaging/nxb.md` and tasks that define stop conditions and proof.
