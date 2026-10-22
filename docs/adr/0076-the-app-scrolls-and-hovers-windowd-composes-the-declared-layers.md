<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0076: The app scrolls and hovers; windowd composes the layers it declared — one scroll physics, one hover decision, no app raster for either

- Status: Accepted
- Date: 2026-10-21
- Links:
  - Tasks: `tasks/TASK-0341-surface-layers-scroll-layers-and-wash-layers.md` (execution + proof)
  - RFCs: `docs/rfcs/RFC-0109-surface-layers-the-app-scrolls-and-hovers-the-compositor-composes.md`
    (the contract: the surface's layers, the two frames, the composition rule, the bounds)
  - Related ADRs: `docs/adr/0075-the-compositor-never-waits-on-a-client.md` (the motion frame is
    state the newest supersedes), `docs/adr/0042-cross-process-surface-transport.md` (the surface
    VMO and the present this extends), `docs/adr/0068-modal-semantics-in-the-dsl-runtime-windowd-keeps-one-routing-verb.md`
    (the hit-test inside a surface is the DSL runtime's)

## Context

Two scroll mechanisms lived side by side. windowd ran the scroll physics of one full-width
three-slice band per window and pushed the position to the app (`INPUT_KIND_SCROLL_POS`); app-host
ran the same physics (`animation::ScrollMomentum`) for every other page and re-rastered the
viewport per step. On the board (cycle 56) no page fits the band — settings has statics beside
its pane, chat's transcript exceeds the band cap, the launcher is on the desktop surface — so
every scroll step and every hover was an app re-raster plus a copy of those rows into the atlas.
The governing boundary map already decided the owner ("windowd = compositor, layer tree,
present — nothing else"; "what a click or a scroll DOES — never in windowd"), and the materials
rules already named the hover's form ("the opacity of a wash layer the compositor owns, not pixels
the app repaints"). Neither was built.

## Decision

- **The app owns scroll and hover behaviour.** app-host runs the one scroll physics per
  `.scroll(...)` container (wheel, fling, paging, end triggers) and the hover decision (the DSL
  runtime's hit-test); it answers with motion — offsets and wash layers — and with band pixels
  only for content entering a band's margin.
- **windowd composes what the app declared.** A surface is a base layer, scroll layers (a viewport
  and a virtual band each), material regions and wash layers (RFC-0109). windowd validates the
  declaration against the surface, routes input to the surface under the pointer, composes the
  layers at the declared offsets and opacities, and never runs a physics or decides a hover.
- **The old paths go in the same package, with a gate against their return**: windowd's scroll
  physics and its band (`forward_body_scroll`, `advance_app_scrolls`, `composite_scrollable_glass`,
  `band_map`), `INPUT_KIND_SCROLL_POS` (4) and `OP_SURFACE_LAYERS` (15) — numbers reserved forever —
  the create frame's band geometry, app-host's band negotiation and per-step viewport re-raster,
  and the paint-time hover wash.
- **Out of scope**: lists and grids emitting only the items a band shows, motion as layer
  properties, zero-copy surfaces (follow-ons named in TASK-0341).

## Consequences

- **Positive**: a scroll step and a hover cost no app raster and no windowd copy; every page can
  scroll, several regions per page, on the desktop surface too; one physics, so the feel is one;
  windowd shrinks.
- **Negative / accepted cost**: a busy app ticks its physics late — the app fills its band in
  time slices so its own raster never delays the tick, and windowd never waits on it (ADR-0075);
  a surface's store holds its bands (about one more window of rows, bounded by the create).
- **Follow-ups**: the collections package (emission by the band's range, anchors by key,
  prepared-text extents, QuerySpec paging), motion as layer properties, a top layer for a
  non-modal box above a scroll layer when an app needs one.

## Alternatives considered

- windowd keeps the physics and scrolls while the app is busy (the compositor-thread model):
  widget behaviour in the compositor and a second physics beside the app's — rejected by the
  boundary map; the app's time-sliced band answers the busy-app case.
- Widen the three-slice band: it cannot express statics beside a viewport, several viewports or a
  horizontal one, and a full-content band cannot hold a long list — rejected for the ring.
