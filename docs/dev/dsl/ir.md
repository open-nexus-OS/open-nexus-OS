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

## Retained identity posture

The IR should preserve enough stable identity for retained measurement and placement caches:

- layout-relevant nodes should carry stable ids / subtree hashes where the runtime needs reuse,
- list-like regions should preserve stable child keys,
- equivalent recompilations must not change identity purely because of incidental formatting or file traversal order.

## Field classification

IR producers and consumers should distinguish between:

- **layout-affecting fields**: constraints, text content, typography, visibility, width-bucket-sensitive structure,
- **paint-only fields**: color/token/opacity and similar values that should not force remeasurement by default,
- **semantics/a11y fields**: accessibility labels/roles and related metadata that affect semantics but not geometry unless
  a task explicitly says otherwise.
