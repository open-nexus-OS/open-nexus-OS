<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Goldens

Goldens are deterministic proof artifacts (images, hashes, or derived views) used to prevent UI drift.

## What counts as a golden

- snapshot PNG (pixel-exact preferred)
- stable hash of snapshot output
- deterministic derived views (e.g., `.nxir.json` for IR debug)

## Guidelines

- keep fixtures small and bounded
- avoid nondeterminism (wallclock, random seeds, host locale)
- store goldens near the feature’s tests (or in a well-known `goldens/` subtree)

## TASK-0054 BGRA snapshot goldens

`TASK-0054` / `RFC-0046` uses repo-owned goldens under
`tests/ui_host_snap/goldens/` for the host CPU renderer proof floor.

Rules:

- equality is based on canonical decoded BGRA8888 pixels,
- PNG files are deterministic artifacts only,
- PNG metadata such as gamma or iCCP chunks must not affect equality,
- normal test runs must not rewrite goldens,
- updates require an explicit `UPDATE_GOLDENS=1` run,
- update paths must remain under the approved golden/artifact root.
