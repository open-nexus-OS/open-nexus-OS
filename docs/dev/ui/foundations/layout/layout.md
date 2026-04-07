<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Layout

Layout aims to be:

- deterministic (stable rounding and ordering),
- cross-device (size classes),
- efficient (bounded per-frame work).

Default posture:

- keep text preparation, measurement, and placement conceptually separate,
- prefer narrow invalidation over full subtree relayout,
- and make resize/scroll updates cheap enough for host goldens and QEMU bring-up.

See also:

- `docs/dev/ui/foundations/layout/layout-pipeline.md`
- `docs/dev/ui/foundations/layout/wrapping.md`
