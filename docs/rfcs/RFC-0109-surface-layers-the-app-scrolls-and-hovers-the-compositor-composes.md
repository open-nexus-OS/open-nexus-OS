<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# RFC-0109: Surface layers — the app scrolls and hovers, the compositor composes what it declared

- Status: Complete (2026-10-22, board cycles 57–59)
- Owners: @ui @runtime
- Created: 2026-10-21
- Last Updated: 2026-10-21
- Links:
  - Tasks: `tasks/TASK-0341-surface-layers-scroll-layers-and-wash-layers.md` (execution + proof)
  - ADRs: `docs/adr/0076-the-app-scrolls-and-hovers-windowd-composes-the-declared-layers.md`
    (the boundary decision), `docs/adr/0075-the-compositor-never-waits-on-a-client.md` (the
    delivery classes the new frames ride), `docs/adr/0042-cross-process-surface-transport.md`
    (the surface VMO and the present)
  - Related RFCs: `docs/rfcs/RFC-0067-windowd-compositor-service-boundary-rasterizer-app-ui-extraction.md`
    (R1 material regions, R2 hover as a presentation modifier — this RFC builds both),
    `docs/rfcs/RFC-0101-gpu-driver-split-contexts-vm-batches-fence-and-the-scanout-contract.md`
    (§8: one frame producer, the planner — amended here: layers keyed by identity, a glass keyed
    by what lies under it), `docs/rfcs/RFC-0057-ui-v3a-layout-engine-pretext-contract.md`
    (scroll = place-only), `docs/rfcs/RFC-0108-ux-latency-contract-instant-and-smooth-as-numbers.md`
    (the budgets a scroll step and a hover are judged by)
  - Docs: `docs/dev/ui/windowd-cleanup-map.md` ("App-Scroll ist App-Sache"),
    `docs/dev/ui/foundations/visual/materials.md` (rule 3: state is a wash layer's opacity)

## Status at a Glance

- **Phase 0 (the measured problem)**: ✅ 2026-10-21 (board cycle 56: every scroll step of every
  app is an app re-raster of the viewport; a hover is an app re-raster of full-width rows)
- **Phase 1 (the compositor composes the declared layers)**: ✅ 2026-10-22 (host tests; the
  store as its own atlas region, the frame's layers shared max-min fair)
- **Phase 2 (the app scrolls and hovers through its layers; the components on them)**: ✅
  2026-10-22 (QEMU `visible` and `headless`; every scrolling page declares its layers)
- **Phase 3 (the board: a scroll step and a hover without an app raster)**: ✅ 2026-10-22 for the
  app's side (cycles 57–59: hover as motion frames copying no rows, a scroll copying only the
  band's new rows, `layers(refused=0)`); the 16.7 ms frame is the frame chain's — cycle 59
  measured a frame waiting 631 ms on gpud's interrupt storm, the next package's

"Complete" means every phase's proof is green on the QEMU lanes and the board lane.

## Scope boundaries (anti-drift)

- **This RFC owns**: what a client surface is made of (a base layer, scroll layers with a
  virtual band each, material regions, wash layers), the two frames that declare them (the
  structure with a present, the motion without one), the compositor's composition rule for each
  kind, and the bounds every one of them is validated against.
- **This RFC does NOT own**: the scroll physics and the hover decision (the app's — the shared
  `animation::ScrollMomentum`, the DSL runtime's hit-test), which items a list emits for a band
  (TASK-0341's follow-on: emission by the band's range, anchors, prepared-text extents, QuerySpec
  paging — the collections contract), interaction and declared motion as layer properties
  (follow-on), zero-copy client surfaces (G5), the GPU executors' programs (RFC-0101).

### Relationship to tasks (single execution truth)

- TASK-0341 proves Phases 1–3.

## Context

Measured on the board (cycle 56, 2026-10-21): no app scrolls through the compositor. The one
compositor scroll path — a full-width three-slice band (fixed header, scrolling body, fixed
footer) with windowd running the scroll physics — fits no real page: settings has a sidebar
beside its scrolling pane (`statics beside the viewport, plain-path fallback`), chat's transcript
is taller than the band cap (`band too tall, plain-path fallback`), the shell's launcher is a
desktop surface, which never had a band. Every scroll step of every app is therefore an app-side
re-raster of the whole viewport, a copy of those rows into the atlas (13–50 µs a row on the board)
and a GPU pass over them. A hover is the same: the app re-rasters the full-width rows of the old
and the new hovered control to paint a wash. Two scroll physics exist side by side (windowd's for
the band, app-host's for everything else), against the governing boundary map: windowd owns
hit-test and routing, "what a click or a scroll DOES — never in windowd".

The reference platforms scroll and highlight without repainting content: the content is a
retained layer the compositor moves within a clip, the highlight a layer whose opacity changes.
This RFC makes that the only way a surface scrolls or shows a hover here.

## Goals

- A scroll step costs the app no raster and windowd no copy: the compositor composes the scroll
  layer's band at the new offset. The app rasters only content that enters the band's margin.
- A hover costs no raster and no copy: a wash layer's rectangle and opacity change.
- Every `.scroll(...)` container of every page is a scroll layer — several per page, vertical,
  horizontal and paged, beside statics, inside panes, on the desktop surface — lists, grids and
  free content alike.
- One scroll physics (the app's), one hover decision (the app's), one composition (windowd's).
- Content of any length: the band is a ring over content space, never the content's full extent.

## Non-Goals

- Lists and grids that emit and lay out only the items a band shows (the follow-on package; the
  band's valid range is the window it reads).
- Motion as layer properties (hover grow, press dip, declared animations) — the follow-on; the
  wash layer is this RFC's one state layer.
- A box above a scroll layer that is not modal (a floating button over a list): no app has one;
  its form (a top layer) is recorded, not built.
- Zero-copy client surfaces (the GPU sampling the app's buffer).

## Constraints / invariants (hard requirements)

- **windowd composes only what a client declared, inside that client's surface.** Every
  rectangle of the structure (viewports, bands, patches, regions) is validated against the
  surface's width, height, rows and store width; a band lies below the base rows, inside the
  store's columns; bands and patches never overlap each other. A frame that fails any check is refused whole (`SURFACE_STATUS_MALFORMED`)
  and the previous structure stays.
- **A slice never shows content the band does not hold.** The compositor composes the visible
  window `[offset, offset + viewport)` intersected with the band's valid range — never rows of
  the ring outside it (stale content of an evicted range).
- **The structure takes effect with the next present of its surface** (atomic with the pixels it
  names); the motion takes effect at once (the next composed frame).
- **windowd runs no scroll physics and makes no hover decision.** The wheel is routed to the
  surface under the pointer like any input; the app answers with motion.
- **Bounded**: 4 scroll layers, 4 wash layers, 4 patches, 16 regions per surface; 3072 surface
  rows; the frame planner's set holds 64 layers; windowd never encodes more.
- No `unwrap`/`expect` on a decoded frame; every decode length-exact and fail-closed.

## Proposed design

### Contract / interface (normative)

**The surface.** A client surface is `width × rows` pixels (BGRA, straight alpha): rows
`[0, height)` are the **base layer** — the page at identity without the content of its uncovered
scroll layers; rows `[height, rows)` are the client's **store**, where its bands and patches live
at places it chooses, in columns `[0, store_w)`. `OP_SURFACE_CREATE` (8) carries `rows` (u16,
`height ≤ rows ≤ 3072`) and `store_w` (u16, `≤ width`; `0` exactly when `rows = height`) instead of
the retired band geometry (`content_h`, `header_h`, `footer_h`).

**Placement.** windowd places the base and the store as two atlas regions at create — the store
`store_w × (rows − height)`, wherever the atlas has room — so a store that grows (a page gaining
a layer, the launcher opening on the desktop) never needs the rows next to the base, and the base
keeps its place across a re-create. With no room for the store the create still succeeds:
`SURFACE_STATUS_NO_STORE` (6), the ack's value the surface id — the surface is its base alone,
the client declares no band and no patch and paints its scroll layers flat (`apphost: store
refused`, once) until its next create. A re-mount (a minimized or covered window shown again) is
not a create: without room for both regions the window stays hidden, as without room for its
base. Measured before this rule (2026-10-22, the allocator replayed on the board's window sizes):
one region per surface failed the desktop's re-create with two windows open and the launcher
opening.

**A scroll layer** is a viewport (the `.scroll(...)` container's clip, base coordinates), an axis
(vertical, horizontal — a paged container is horizontal to the compositor) and a **virtual band**:
a ring of `capacity = strip_len × strips` positions along the axis, stored as `strips` strips of
`strip_len` positions each, placed side by side across the axis from the band's origin
(`bx`, `by` — vertical strips are `viewport_w` wide and `strip_len` rows tall, next to each other;
horizontal strips are `strip_len` columns wide and `viewport_h` rows tall, under each other).
Content position `p` (along the axis, `0` = the viewport's leading edge at offset 0) lives at ring
position `p mod capacity`. The band's **valid range** `[valid_start, valid_start + valid_len)`
(`valid_len ≤ capacity`) is the content the ring holds now; `valid_len = 0` means the layer is
**covered** (a modal box over it: the app paints its content into the base at the frozen offset,
the compositor draws no slice). A band whose every pixel is opaque says so (`BAND_OPAQUE`): its
slices occlude what lies below.

**A wash layer** is a patch spread over a rectangle with a corner radius and an opacity, over the
content of its parent layer (the base, or scroll layer `k`, whose offset it follows and whose
viewport clips it). A **patch** is a 16×16 square of the store whose 14×14 interior is one colour
(the TASK-0336 patch; the opacity carries the colour's alpha).

**`OP_SURFACE_TREE` (33)**, client → windowd, the structure, applied with the next present:
`surface_id:u32, regions:u8, scrolls:u8, patches:u8, reserved:u8`, then `regions × 12` bytes (the
`LayerDesc` of the retired `OP_SURFACE_LAYERS`), `scrolls × 24` bytes
(`vx,vy,vw,vh:u16, axis:u8, flags:u8, bx,by,strip_len:u16, strips:u8, reserved:u8,
valid_start:u32, valid_len:u16`), `patches × 4` bytes (`px,py:u16`).

**`OP_SURFACE_MOTION` (34)**, client → windowd, the properties, applied at once (the newest
supersedes): `surface_id:u32, scrolls:u8, washes:u8, reserved:u16`, then `scrolls × 4` bytes
(`offset:i32` along the axis) and `washes × 16` bytes (`parent:u8` — 0 the base, `k` scroll layer
`k`; `patch:u8, radius:u8, opacity:u8, x:i32, y:i32` — content coordinates of the parent,
`w,h:u16`).

**Retired, numbers reserved forever:** `OP_SURFACE_LAYERS` (15 — its regions ride the tree),
`INPUT_KIND_SCROLL_POS` (4 — the app knows its offset).

**Composition (windowd, per surface, back to front):** the base (its window body, glass or
solid); the material regions; each uncovered scroll layer's slices — the visible window cut at
strip and ring boundaries, each piece one `CompositeLayer` from its band place to its viewport
place; the wash layers. The desktop surface draws a scroll layer or a wash in the pass of the
region holding it (a panel-level region's above the windows). Every layer carries a key unique
in the frame (its surface, its kind, its index): the planner matches a layer to its last frame's
by key, never by its place in the list.

**The planner (RFC-0101 §8 amended):** a layer keeps its glass cache slot, its cleaned rectangle
and its damage history by key; a glass's backdrop key folds only the layers whose extent reaches
it — a slice moving elsewhere re-blurs no glass.

**The app (app-host):** one `ScrollMomentum` per scroll layer (wheel, fling, paged glide and its
page triggers, `EndReached` scoped to the container that reached its end), the wheel targeted at
the scroll layer under the pointer; the band filled ahead of the viewport within a time budget
per frame pulse, nearest content first, never blocking the physics tick; motion sent on every
tick that moved; the structure and the presented store rows sent together. The hover decision
stays the DSL runtime's hit-test, answered with a wash.

### Phases / milestones (contract-level)

- **Phase 0**: the problem measured (cycle 56).
- **Phase 1**: the wire, the planner's keys and glass keys, windowd's composition and the retired
  paths gone — host tests (`test_reject_*` for every bound).
- **Phase 2**: app-host on scroll layers and wash layers; the layout's scroll ownership; every
  component that scrolls on them; the QEMU lanes.
- **Phase 3**: the board: a scroll step's frame inside 16.7 ms, no app raster per step beyond the
  band's edge, a hover without a raster.

## Security considerations

- **Threat model**: a client (any app) declares a structure that makes windowd sample another
  client's atlas rows, the compositor's own planes, or memory past the atlas; or exhausts the
  frame's layer budget so other windows vanish.
- **Mitigations**: every rectangle is checked against the declaring surface's own bounds before
  it is kept, so every sampled pixel lies in that surface's atlas region; counts are bounded per
  surface, and the planner's set is shared max-min fair after the window bodies (a client over
  the bound loses its own last layers, never another window's —
  `test_reject_a_surface_crowding_out_another_window`); identity is the sender id the kernel
  stamps on the frame, checked against the one captured at the surface's create
  (`sender_service_id`, `test_reject_layers_for_a_surface_the_sender_does_not_own`), never a
  payload field.
- **Open risks**: a client that declares a band opaque when it is not draws its own wrong pixels
  (no cross-client effect).

## Failure model (normative)

- A malformed or out-of-bounds tree or motion frame: refused whole, the last valid structure and
  motion stay, `windowd: surface tree refused (id= reason=)` (`surface motion refused` for a
  motion) once per surface; every refusal is counted on windowd's loop line
  (`layers(refused=N)`). A refused motion drops its offsets with its wash, so the client never
  sends one windowd would refuse: it checks each motion against the structure it last sent and
  leaves out a wash that would not pass (no wash for a box without area — board cycle 57, where
  one empty wash stalled a surface's scrolling).
- A band behind the viewport (the app late): the uncovered rest shows the base's pixels at that
  place (the container's own background) until the band fills — never stale content.
- More layers than the frame holds: the window bodies and the desktop's base and bar first, then
  each surface's declared layers within its max-min fair share of the rest — while the frame
  holds them all nothing is dropped; past it the surfaces declaring most lose their own last
  layers (in encode order: regions, slices, washes). Said once
  (`windowd: layers over the frame bound (dropped=)`), never a refused present.
- More scroll layers than a page may declare (`MAX_SCROLL_LAYERS`): the first ones in layout
  order are layers, the rest flatten into the base (`apphost: scroll layers over the bound
  (dropped=)`, once).

## Proof / validation strategy (required)

### Proof (Host)

```bash
cargo test -p nexus-display-proto
cargo test -p nexus-gfx --test frame_plan --test frame_motion --test frame_board --test frame_identity
cargo test -p windowd
cargo test -p app-host
cargo test -p nexus-layout -p nexus-scene-raster -p nexus-dsl-runtime
just test-host
```

Negative cases — the wire (`surface_layers`, `client_surface`):
`test_reject_a_tree_longer_than_its_counts`, `test_reject_more_scroll_layers_than_the_bound`,
`test_reject_a_band_outside_the_surface`, `test_reject_a_band_over_the_base`,
`test_reject_overlapping_bands`, `test_reject_a_patch_outside_the_store`,
`test_reject_a_viewport_or_region_outside_the_base`,
`test_reject_short_strips_and_a_valid_range_past_the_ring`,
`test_reject_a_wash_naming_a_missing_patch`, `test_reject_a_create_with_rows_below_its_height`,
`test_reject_a_store_wider_than_the_surface_or_without_rows`,
`test_reject_a_band_past_the_declared_store`;
windowd: `test_reject_a_surface_whose_rows_are_below_its_height_or_past_the_cap`,
`test_reject_a_store_wider_than_the_surface_or_without_rows`,
`test_reject_a_place_outside_the_placed_regions`,
`test_reject_showing_content_outside_the_valid_range`,
`test_reject_layers_for_a_surface_the_sender_does_not_own`,
`test_reject_a_surface_crowding_out_another_window`; app-host:
`test_reject_keeping_a_range_the_window_left`.

### Proof (OS/QEMU)

```bash
just test-os visible
just test-os headless
```

Required: both lanes green with their wheel pulse (`SELFTEST: ui visible wheel ok`); a page
with a `.scroll(...)` says `apphost: scroll layers (n= band_rows=)` at its mount (a measurement
line, no `ok`); no `surface tree refused`, `surface motion refused` or `layers over the frame
bound` line.

### Proof (board)

Read from the lines every lane already prints, during a scroll and during a hover, against
cycle 56: `windowd: loop … frames(moved=) work(copy_rows=)` — a hover and a scroll step inside
the band's margin are `moved` frames that copy no rows (the app rastered nothing; every row an
app rasters reaches windowd as a copied row), a scroll that reaches the margin copies only the
band's new rows; `gpud: gpu frame us` per frame inside 16.7 ms; `windowd: ux input` (RFC-0108).
A QEMU rung that wheels over a scroll layer and judges the same counters is TASK-0341's
follow-up.

## Alternatives considered

- **windowd keeps the scroll physics (the compositor scrolls asynchronously).** Smooth while the
  app is busy, but widget behaviour (paging, end triggers, nested viewports) in the compositor —
  against the boundary map — and a second physics beside the app's. Rejected; the app's band is
  filled in time slices so its tick never waits on a raster.
- **The whole content in one tall band.** Bounded by the atlas; chat's transcript already
  exceeds it. Rejected for the ring.
- **Fixed tiles with a map.** More layers per frame and per-tile bookkeeping for what a ring with
  a valid range says in two numbers. Rejected.
- **The wash under the text.** Needs the text in its own layer (or the band transparent, four
  times the board's per-pixel cost of an opaque band). The wash is the text's own tone at low
  alpha, so over the content it looks the same; it stays over.

## Open questions

- Pressure: a band is purgeable memory (RFC-0100 §purgeable) — dropped first, refilled from the
  layout.

## RFC Quality Guidelines (for authors)

When writing this RFC, ensure:

- Scope boundaries are explicit; cross-RFC ownership is linked.
- Determinism + bounded resource use are specified in Constraints section.
- Security invariants are stated (threat model, mitigations, DON'T DO).
- Proof strategy is concrete (not "we will test this later").
- If claiming stability: define ABI/on-wire format + versioning strategy.
- Stubs (if any) are explicitly labeled and non-authoritative.

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 0**: the problem measured — proof: cycle 56 (`docs/board/measurements/2026-10-21-gpu-vendor-per-pixel/README.md`)
- [ ] **Phase 1**: the compositor composes the declared layers — proof: host tests above
- [ ] **Phase 2**: the app scrolls and hovers through them — proof: QEMU lanes above
- [ ] **Phase 3**: the board — proof: cycle 57 lines
- [ ] Task linked with stop conditions + proof commands.
- [ ] QEMU markers appear in `scripts/qemu-test.sh` and pass.
- [ ] Security-relevant negative tests exist (`test_reject_*`).
