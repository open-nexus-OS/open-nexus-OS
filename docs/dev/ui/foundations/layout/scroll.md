<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Scroll

How a page scrolls. Contract: `docs/rfcs/RFC-0109-surface-layers-the-app-scrolls-and-hovers-the-compositor-composes.md`;
boundary: `docs/adr/0076-the-app-scrolls-and-hovers-windowd-composes-the-declared-layers.md`.

Scroll behaviour is smooth and bounded (a step costs no app raster; every buffer has a cap),
deterministic where tests assert it (the band planner and the composition math are host-tested),
and owned by the app: windowd routes the wheel and composes, it runs no physics.

## The model

- Every top-level `.scroll(vertical|horizontal|paged)` container is a **scroll layer**. The layout
  places its content once, at identity: each content box carries the container as its
  `scroll_root`, clips inside the content stay the content's own, and the viewport clips in the
  compositor, not in the layout.
- app-host paints the content into the layer's **band**: a ring of strips in the surface's store
  (the rows below its base), holding the visible window and a margin on each side. Content enters
  the band in time slices (at most 4 ms a pass), the visible window first, then the margins.
- A wheel notch, a fling or a page step moves the layer's one physics
  (`animation::ScrollMomentum`). Each frame the app sends the new offset as motion
  (`OP_SURFACE_MOTION`): no present, no raster, no copy. windowd composes the band's visible
  window, in at most three pieces (where the ring wraps and where a strip ends), and shows only
  content the band holds; the rest of the viewport shows the container's own background until
  the band fills.
- A hover is a **wash layer**: a 16×16 patch of the theme's hover colour drawn into the store once,
  spread by windowd over the hovered control at the wash's opacity (materials rule 3).

## Writing pages

- Put `.scroll` on the container that scrolls, beside any statics (headers, sidebars, toolbars).
  A page may have several: four are layers; more are painted into the page unscrolled, clipped to
  their viewport (`apphost: scroll layers over the bound (dropped=)`).
- A `.scroll` inside another scroll container's content clips like `.overflow(hidden)` and does
  not scroll on its own — one scroll root per content tree.
- `.overflow(hidden)` clips wherever its container is painted, inside scrolling content too.
- An `.overlay()` (menu, sheet, picker) over a scroll layer covers it: the layer's content paints
  into the base under the overlay, at its offset, until the overlay closes.
- `on EndReached -> dispatch(...)` on the container loads more: it fires once when the offset
  nears the content end and re-arms after each layout; the page lays out again and the band
  redraws the rows the change touched.
- Sizing: a vertical scroller gives up its height and keeps its width; a horizontal or paged one
  gives up its width and keeps its height. Children along the scroll axis keep their sizes (a
  pager never squeezes its pages).

## Not yet (TASK-0341 follow-ons)

- Lists and grids still lay out every item. The band's valid range is the window the collections
  package will emit items for — with prefetch, anchors by key, prepared-text extents and
  QuerySpec paging.
- A drag scrolls nothing: it reaches the DSL's drag triggers only.
- Press, selection and focus as wash opacity; hover grow and press dip as layer properties.

## Proof

- Host: `cargo test -p windowd` (`layer_geometry`: pieces at the wrap and the strips, the valid
  range, the frame's layer bound), `cargo test -p app-host` (`band_math`: fill order, ring
  eviction), `cargo test -p nexus-layout` (`scroll_root_tests`).
- Lanes: `apphost: scroll layers (n= band_rows=)` at a scrolling page's mount.
- Board: a scroll step and a hover are `moved` frames on `windowd: loop … frames(moved=)
  work(copy_rows=)` with no copied rows unless a band's margin fills; `gpud: gpu frame us` inside
  16.7 ms (RFC-0108).
