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
