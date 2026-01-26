<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Scene IR (`.nxir`)

The DSL lowers source (`.nx`) into a deterministic Scene IR.

## Formats

- **Canonical**: `.nxir` (Cap'n Proto; deterministic, bounded parsing)
- **Derived**: `.nxir.json` (deterministic view for host goldens/debug)

## Determinism rules

- stable ordering for maps/lists in IR
- stable ids/hashes where required
- no host-dependent timestamps
