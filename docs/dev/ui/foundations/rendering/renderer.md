<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Renderer

This document captures renderer-facing contracts used by UI:

- scene submission
- text rendering hooks (shaping/fallback)
- deterministic rasterization constraints for goldens

## Responsibility split

By default:

- the runtime decides invalidation scope and stage ordering,
- the text stack owns prepared text and line-layout generation,
- the layout system owns measurement and placement decisions,
- the renderer consumes immutable scene-submission inputs and performs raster/composition work.

The renderer should not become the hidden owner of layout caches or retained-tree mutation.

## Backend posture

Different backends may render differently, but should preserve the same pipeline contract:

- DOM: avoid forced layout reads and unnecessary node churn,
- Canvas: treat scene submission as the retained caller-owned plan,
- SVG: update affected geometry/text attributes instead of rebuilding whole subtrees,
- NativeWidget: expose shell-facing bounds and damage contracts even when the inner renderer is specialized.

See also: `docs/dev/ui/foundations/layout/layout-pipeline.md`
