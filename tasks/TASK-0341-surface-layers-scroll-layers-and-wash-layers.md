---
title: TASK-0341 Surface layers — every `.scroll(...)` is a scroll layer the compositor moves, every hover a wash layer; the app owns both behaviours, windowd composes
status: Done (2026-10-22, board cycles 57–59)
owner: @ui
created: 2026-10-21
depends-on: [TASK-0340 (the frame without the retained-plane rewrite), TASK-0339 (the compositor never waits on a client)]
follow-up-tasks: [a QEMU rung that wheels over a scroll layer and judges `frames(moved=)` / `copy_rows=`; the collections package — List/Grid emit and lay out only the items a band's valid range shows (+ prefetch), anchors by key, prepared-text extents (RFC-0057), QuerySpec paging on the band's range, flex wrap; motion as layer properties (hover grow, press dip, declared animations — the raster NodeAnim goes); zero-copy client surfaces (G5); a top layer for a non-modal box over a scroll layer]
links:
  - RFC: docs/rfcs/RFC-0109-surface-layers-the-app-scrolls-and-hovers-the-compositor-composes.md
  - ADR: docs/adr/0076-the-app-scrolls-and-hovers-windowd-composes-the-declared-layers.md
  - Measurement: docs/board/measurements/2026-10-21-gpu-vendor-per-pixel/README.md ("Cycle 56")
  - Boundary: docs/dev/ui/windowd-cleanup-map.md (input boundary), docs/dev/ui/foundations/visual/materials.md (rule 3)
---

## Context (cycle 56)

No page scrolls through the compositor: settings (`statics beside the viewport`), chat (`band too
tall`) and the launcher (a desktop surface) all fall back to the app re-rastering the viewport per
scroll step; a hover re-rasters the full-width rows of two controls. Two scroll physics exist
(windowd's band, app-host's plain path). GPU frames average 29–36 ms (`gpud: gpu frame us`), the
cache clean 0.9–1.7 ms of it (not the bottleneck); `windowd: ux input` median 842 ms.

## Goal

Every `.scroll(...)` container of every page is a scroll layer: its content rendered once into a
virtual band (a ring over content space), composed by windowd at the offset the app's one physics
commits. Every hover is a wash layer. No app raster per scroll step beyond the band's edge, none
per hover; windowd runs no physics.

## Non-Goals (this task)

- Item-range emission for lists/grids, anchors, prepared-text extents, QuerySpec paging, flex
  wrap (the collections package — the band's valid range is the window it will read).
- Motion as layer properties (hover grow, press dip, toggle thumb, declared animations stay the
  raster's `NodeAnim` until that package replaces it whole).
- Zero-copy surfaces; per-write atlas upload (the clean measured 0.9–1.7 ms a frame).
- A non-modal box above a scroll layer (no app has one).

## Architecture verdict (three lenses)

- **Scope** — in: `source/libs/nexus-display-proto` (tree/motion/create, retirements),
  `userspace/nexus-gfx` (layer key, planner keyed by key, glass keyed by its reach, 64 layers, the
  CPU blit's spread + opacity), `source/drivers/gpud` (executors keyed by key, virgl band upload,
  no permanent desktop-off on a draw overflow), `source/services/windowd` (tree/motion storage +
  validation, one surface-layer encoder for windows and the desktop, copy only presented rects;
  the band, its physics and `SCROLL_POS` deleted), `source/services/app-host` (scroll layers:
  physics per container, bands filled in time slices, base/band raster split, per-layer hit-test,
  wheel at the pointer, scoped end triggers, wash layers; the band negotiation, the per-step
  viewport re-raster and the paint-time wash deleted), `userspace/ui/layout` + `layout-types`
  (scroll ownership stamped, content clips relative to their scroll root, measure by axis, no
  shrink along a scroll axis, a nested scroll container clips), `userspace/dsl/runtime`
  (`.overflow(hidden)` clips, layered hit-test, scoped triggers), `userspace/ui/scene_raster`
  (clip always honoured, per-layer scroll views, the hover wash gone), components (settings
  NavSidebar scrolls, stash's listing scrolls), docs. Out: the follow-ons above.
- **Invariant** — windowd samples only the declaring surface's atlas region and never shows ring
  content outside the valid range: every rect validated (`test_reject_a_band_outside_the_surface`,
  `test_reject_a_band_over_the_base`, `test_reject_overlapping_bands`,
  `test_reject_a_patch_outside_the_store`, `test_reject_a_wash_naming_a_missing_patch`,
  `test_reject_more_scroll_layers_than_the_bound`, `test_reject_a_tree_longer_than_its_counts`,
  `test_reject_a_create_with_rows_below_its_height`); the retired names gated.
- **Contract** — RFC-0109 (new), ADR-0076 (new), RFC-0101 §8 amended (planner keys), RFC-0067
  R1/R2 built.

## Plan

- **P1 wire + planner** — create `rows`; `OP_SURFACE_TREE` (33), `OP_SURFACE_MOTION` (34); 15 and
  `INPUT_KIND_SCROLL_POS` retired; `CompositeLayer` gains a key word; the planner and the glass
  cache match by key; a glass's key folds only layers within its reach; `MAX_LAYERS` 64; the CPU
  blit spreads a band and honours opacity; virgl uploads a scroll band's whole rows.
- **P2 windowd** — tree pending → active at present; motion at once; one encoder for a surface's
  layers (body, regions, slices, washes) used by app windows and the desktop; presented rects
  copied alone; the band, the physics, `SCROLL_POS`, `band_map`, `composite_scrollable_glass`
  deleted; the wheel routed to every body.
- **P3 layout + DSL** — scroll ownership, content clips, measure by axis, no shrink, nested →
  clip, `.overflow(hidden)` → `Hidden`, layered hit-test, triggers scoped to their container.
- **P4 app-host** — scroll layers, bands, motion, wash layers; deletions.
- **P5 components + gates + docs** — NavSidebar `.scroll(vertical)`, stash listing
  `.scroll(vertical)`, comments that cite "one compositor scroll region" fixed; retired-name gate;
  markers; docs sweep.

## Proof

- Host: the negative tests above; planner (`frame_plan`, `frame_motion`, `frame_board`): a slice
  moving re-blurs no glass outside its reach, a layer inserted mid-list leaves every later layer
  clean; windowd composition math (ring cut at the wrap and the strips, valid-range clip, wash
  follows its parent's offset); layout (ownership, clips, measure by axis); app-host band planner.
- Host, the frame's bound: past the planner's set the surfaces declaring most lose their own
  last layers, never another window's (`test_reject_a_surface_crowding_out_another_window`);
  nothing is dropped while the frame holds every layer.
- QEMU: `visible` and `headless` green with their wheel pulse; `apphost: scroll layers (n=
  band_rows=)` at a scrolling mount; no `surface tree refused`, `surface motion refused` or
  `layers over the frame bound` line.
- Board cycle 57, read from `windowd: loop … frames(moved=) work(copy_rows=)` during a scroll and
  a hover: a hover and a scroll step inside the band's margin are `moved` frames copying no rows
  (no app raster), a scroll reaching the margin copies the band's new rows alone; `gpud: gpu
  frame us` and `windowd: ux input` against cycle 56.

## Results

- 2026-10-21: designed (RFC-0109, ADR-0076).
- 2026-10-22: built, host-green, `just check` + `just diag` + `just dep-gate` + `just test-host`
  green. Found on the way and fixed: a clipping container's `clip_rect` was the clip it imposes
  on its content, so a painter honouring it cut the container's own padding (and the hit-test
  refused taps there) — the layout now keeps the two apart (`LayoutBox::content_clip`); a store
  that could not hold every band blanked every layer (now the longest goes flat); a page with
  more than four scroll containers painted the rest unclipped (now flat and clipped, as the
  hit-test places them); a timer-driven relayout presented before the store was re-planned (now
  planned at the relayout). Measured with windowd's allocator replayed on the board's window
  sizes: one atlas region per surface (base + store) failed the desktop's re-create with two
  windows open and the launcher opening, and a third window could not open — now the store is a
  region of its own (its used width), the CPU-only blur band is not allocated where the GPU
  composes the glass, and a create without room for the store answers `NO_STORE` (the app scrolls
  flat, named) instead of failing; three 1440×853 windows and the launcher fit, a fourth window's
  store may not. The frame's 64 layers are shared max-min fair between surfaces.
- 2026-10-22, board cycle 57 (`docs/board/measurements/2026-10-21-gpu-vendor-per-pixel/`):
  scroll layers declared on every scrolling page, the desktop's store placed when the launcher
  opened, hover as motion frames with no copied rows. RED: a wash of zero size (a hovered box
  without area) made windowd refuse the surface's motions whole — chat's and the launcher's
  scrolling stalled. Fixed in app-host (no empty wash; each motion checked against the structure
  last sent) and windowd counts refusals on its loop line. Cycle 58 next.
- 2026-10-22, board cycle 58: no refusal left. Windows could not be moved — every
  `CONTROL_WIN_*` named the window by a 4-bit surface id, wrong once ids pass 15 (an old bug the
  session reached): windowd now acts on the sender's own window, resolved from the kernel sender
  id (`control_gate::own_window`). Measured: ~3 composed frames a second while anything moves
  (cycles 55–58 alike) — the GPU frame (25–40 ms) is not what paces it; cycle 59 measures gpud's
  late blank wakes and each frame's issue → ack time.
- 2026-10-22, board cycle 59: window moving and tiling work again; no refusal. Measured: a frame
  waits 631 ms (median) from windowd's send to gpud's credit, and gpud serves ~790 blank wakes
  per flip, 85 % outside the blank — an interrupt storm in gpud's vertical-blank setup (six
  sources left unmasked, only the vsync cleared). Not this task's — the next package (the frame
  chain: one interrupt a frame, hop markers, then the measured GPU levers). TASK-0341 Done.
