<!-- Copyright 2024 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Bundle Manager Manifest Schema

The bundle manager crate (`userspace-bundlemgr`) parses application manifests
stored as TOML documents. The host-first implementation focuses on validating
schema correctness and surfacing actionable diagnostics to developers.

## Required fields

| Field      | Type            | Description                                  |
|------------|-----------------|----------------------------------------------|
| `name`     | `string`        | Unique bundle identifier (non-empty).        |
| `version`  | `string` (semver)| Human readable bundle version.              |
| `abilities`| `array<string>` | Declared abilities provided by the bundle.   |
| `caps`     | `array<string>` | Capabilities required by the bundle.         |
| `min_sdk`  | `string` (semver)| Minimum supported NEURON SDK version.       |

Unknown keys are not fatal—the parser records a warning string for each
unexpected entry so build tooling can surface them. String fields are trimmed
and must not be empty, and arrays must contain non-empty string items.

## Errors

The parser returns `userspace_bundlemgr::Error` with the following variants:

- `Toml(String)` – raw TOML parsing failure.
- `MissingField(&'static str)` – a required field was not present.
- `InvalidRoot` – the manifest root is not a TOML table.
- `InvalidField { field, reason }` – a field could not be interpreted (for
  example, a semver parse failure or an empty capability name).

This structured error model allows callers to present precise feedback and
continue execution when warnings (rather than errors) occur.
