---
title: TASK-0070 UI v8b: real window management (z/focus, drag-edge snap, header buttons + dock, backdrop correctness, edge resize + cursor shapes) + rendering quality (runtime text, SVG seams) + scroll unification
status: In progress
owner: @ui
created: 2025-12-23
updated: 2026-07-03 (full rewrite to IST + new scope)
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - UI v6a WM baseline: tasks/TASK-0064-ui-v6a-window-management-scene-transitions.md
  - Layout engine contract: docs/rfcs/RFC-0057-ui-v3a-layout-engine-pretext-contract.md
  - Compositor boundary: docs/rfcs/RFC-0067-windowd-compositor-service-boundary.md
  - Settings track (S-track twin): tasks/TASK-0072-ui-v9b-prefsd-settings-panels-quick-settings.md
  - Testing contract: scripts/qemu-test.sh
---

## Rewrite note (2026-07-03)

This task was a stale draft. The old text assumed snap zones from `TASK-0066` v7a existed
(they never landed — no snap/resize code exists in `windowd`), planned **global keyboard
shortcuts** for snapping (explicitly rejected — pointer-driven edge snapping instead), and
scoped settings overlays that now live in the reshaped `TASK-0072` (settingsd track). This
rewrite states the honest IST and the new combined scope. Phases below execute together with
`TASK-0072` as one boot-gated track.

## Progress ledger

- **Phase 1 DONE (committed)**: window collection on `window_scene::WindowStack` (z/focus/
  raise), scene emission + input hit-testing share `order()`/`hit_order()`, chrome above
  windows, unified wheel routing to the topmost window under the cursor.
- **Phase 2 DONE (committed)**: title-bar `– □ ×` (Frame SSOT for hit-test AND renderer),
  minimize → bottom-center glass dock (visible only while ≥1 minimized), fullscreen state.
- **Phase 3 DONE (committed)**: drag-to-edge snap (left/right halves, top = fullscreen),
  7 px edge/corner resize with deterministic from-start math, vendored resize cursor shapes
  via `OP_UPLOAD_CURSOR` re-send, TRUE full-display fullscreen with size-parametric content.
- **Phase 6 (in review)**: runtime text — build.rs bakes A8 coverage glyph atlases of the
  vendored variable UI face at 13px (chrome labels/dropdown/titles) and 16px (chat body,
  search rows via shared label path, greeter name): ASCII 32–126, per-glyph
  placement/advance, line metrics, and sparse non-zero kerning pairs, all consts
  (`FONT13_*`/`FONT16_*`, ~16 KB total). New crate-root `src/text.rs` (host-tested, 5 tests)
  renders ROW-BASED to match the surface painters: `draw_text_row` blends the slice of a run
  intersecting the current row (band top may be negative for scrolled chat lines), `measure`
  drives topbar cell layout AND hit-testing from one function. Migrated off the 5×7 bitmap:
  chat messages, window titles, topbar items, dropdown entries, search filter + rows, greeter
  name; `compositor/font.rs` shim deleted. The baked 16px line height (20px) equals the old
  bitmap line height, so chat scroll/height math is unchanged; wrapping stays char-count
  (divisor = the face's average advance) until Phase 7's measured wrap. The legacy
  scene-graph TILE-text primitive intentionally stays on the 5×7 table (solid tiles cannot
  express coverage; it backs only the old proof-panel path). Marker:
  `windowd: font family=inter sizes=13,16`; `ui.font.family` key shape prepared (settingsd
  wiring in Phase 10; live face switching stays a follow-up).
- **Phase 5 DONE (committed)** — the visible icon seams had THREE stacked causes in `nexus-svg`;
  all fixed, each with regression proofs:
  1. **Stroke-piece winding cancellation (the dominant artifact)**: segment quads, joins and
     caps share one shape under the nonzero rule, but round-join/cap discs (and turn-dependent
     bevel/miter wedges) were wound OPPOSITE to the segment quads — overlap cancelled the
     winding (+1 − 1 = 0) and punched a hole at every joint. A stroked circle (the `search`
     icon) rendered as a DOTTED ring. Fix: `stroke_piece_edges` normalizes every stroke piece
     to one shared orientation (signed-area check) before emitting. Proof:
     `stroked_circle_ring_has_no_joint_holes` samples all 360° of the ring midline.
  2. **Number lexer ate a second decimal point**: compact path data (`1.099.092` = 1.099 then
     .092) parsed as one invalid token, corrupting every following parameter — the
     `message-circle` bubble arc vanished entirely. Fix: a number accepts at most one `.`.
     Proof: `compact_numbers_and_implicit_arc_repeats_render_the_bubble`.
  3. **Per-shape coverage conflation**: shapes composited one at a time, so abutting shapes
     left alpha < 1 seams at shared fractional edges. Fix: one unified sweep — each sub-row is
     partitioned at ALL shapes' sorted crossings, the covering stack composites ONCE per
     interval (painter's order, premultiplied), rows write back with exact analytic overlap.
     Proofs: abutting-no-seam / translucent-overlap-OVER / opaque-top-hides-bottom.
  Verified end-to-end with a host probe replicating the windowd icon bake (4× SSAA tinted →
  box downscale → unpremultiply): search/message-circle/house/square now render clean.
  Consumer supersampling kept for AA quality; build.rs comments no longer call it a seam
  workaround.
- **Phase 4 DONE (committed)**: GL backend now blurs the **destination-so-far** — before each glass
  layer composites, the RT region beneath it (padded by the radius) is snapshotted via
  `RESOURCE_COPY_REGION` into a screen-sized scratch texture (GPU-only, no guest backing) and
  the existing gaussian pass samples that snapshot instead of the wallpaper texture; layers
  composite back-to-front, so lower windows and chrome are already in the snapshot. Marker
  `gpud: rt backdrop dst ok`. Fullscreen windows composite square (no radius, no shadow) and
  exclude covered floating windows from composition entirely. Known remainders inside this
  phase's scope: (a) CPU-backend parity — its `LayerBackdrop` restore still samples the
  retained wallpaper plane, so glass there keeps the wallpaper backdrop for now; (b) the blur
  rect is square, so the few pixels outside a rounded corner show blurred (not sharp)
  underlying content — a blur-pass corner clip is a polish follow-up.

## IST (verified 2026-07-03; Phases 1-3 above supersede the WM bullets)

- Exactly **two windows** exist, as named fields on the compositor runtime (`chat`, `search`) —
  no window collection, no focus tracking, no click-to-raise.
- **Draw order is hardcoded** in `runtime/scene.rs` (search, then chat) → the chat window always
  renders above the search window regardless of interaction. A pure, host-tested z-order SSOT
  exists at `source/services/windowd/src/window_scene.rs` (`composition_order()`) but is **not
  wired into the runtime**.
- Chrome topbar/dropdown/side panel are emitted **before** the windows (windows draw over the
  topbar); the right-side hamburger/chat buttons are emitted after (above windows).
- **No resize, no snap, no minimize/maximize, no dock/taskbar, no fullscreen state.** Title-bar
  drag + close-x exist (`nexus_widget_window::Frame`, `WindowPress::{Close,TitleDrag,Body,Miss}`).
- **Glass blur and rounded corners reveal the wallpaper**, never the actual underlying window:
  `LayerBackdrop` samples Plane 1, which holds only the wallpaper; windows are independent
  composite layers and are absent from every backdrop source. On the GL backend glass has no
  real gaussian at all (tint + bilinear softness only).
- **Cursor**: only the default pointer shape is baked from the vendored cursor theme
  (`resources/cursors/mocu/src/svg/default.svg`); the theme ships full resize variants
  (`ew/ns/nesw/nwse-resize.svg`) unused. gpud has re-upload plumbing but no shape-select API.
- **Text**: dynamic text (chat messages, search rows) renders with a hand-coded 5×7 bitmap font;
  the vendored Inter face is used only at build time for a fixed set of pre-rendered strings.
- **SVG**: adjacent elements show seams/outlines — per-shape straight-alpha `over` compositing in
  `userspace/ui/svg/src/raster.rs` (conflation artifact); today only mitigated by build-time 4×
  supersampling.
- **Scroll is a double structure**: chat = `nexus-virtual-list` + render-once tall atlas surface +
  GPU source-row offset (`OP_SET_CHAT_SCROLL`); search = raw `ScrollMomentum` + per-row CPU
  re-render. A wheel event over neither window is a silent no-op. `nexus-layout` (the
  deterministic layout engine per RFC-0057) and `nexus-virtual-list` (List/VirtualList/lazy
  `ItemProvider`) are done and host-tested but not uniformly adopted.

## Goal

One production-grade window system plus the rendering/scroll floor it sits on:

1. **Window collection + z-order + focus** (Phase 1): windows live in a collection driven by
   `window_scene::composition_order()`; scene emission AND input hit-testing iterate the same
   order (hit-test = reverse); click-to-raise + focus; chrome topbar renders **above** windows;
   wheel routes to the topmost window under the cursor (no silent no-op).
2. **Header buttons + minimize dock + fullscreen** (Phase 2): title bar gets minimize (–),
   maximize (□), close (×). Minimize hides the window and shows its icon in a bottom-center
   glass dock (dock exists only while ≥1 window is minimized; icon click restores + raises).
   Maximize = fullscreen covering the chrome; restore remembers the floating frame.
3. **Drag-to-edge snap + edge resize + resize cursors** (Phase 3): dragging a window to the
   right/left screen edge snaps it to that half; to the top edge = fullscreen. **No keyboard
   shortcuts.** Edge/corner hit zones (6–8 px) resize floating windows within min-size clamps;
   hovering an edge switches the hardware cursor to the matching vendored resize shape via a
   new `OP_SET_CURSOR_SHAPE` (sprites uploaded once, selected by index).
4. **Backdrop correctness** (Phase 4): glass blur and rounded corners composite over the real
   content beneath (lower windows, chrome), not the wallpaper. Mechanism: destination-so-far —
   composition is back-to-front, so the backdrop source is the composition target at the moment
   the layer composites. The GL backend lands its real gaussian backdrop pass here (the
   `composite_layer_rt` code already reserves the seam). Backdrop caches become generation-keyed
   (invalidated by damage/z-change below). Fullscreen windows may keep the cheap wallpaper
   backdrop.
5. **SVG seam fix** (Phase 5): accumulate the whole SVG into a premultiplied intermediate
   (coverage-weighted source-over per shape), composite onto the destination **once** — kills
   conflation seams for build-time icons and runtime renders.
6. **Runtime text** (Phase 6): build-time A8 coverage glyph atlases of the vendored Inter face
   (13 px + 16 px, glyphs 32–126, metrics/kerning as consts) + a `draw_text_run` runtime module
   replace the 5×7 bitmap in chat/search. Font family is a manifest-driven default with the
   typed settings key shape (`ui.font.family`); live switching is a follow-up.
7. **Scroll unification** (Phase 7): one architecture — layout tree → `List` → `VirtualList`
   (lazy `ItemProvider`) with `ScrollMomentum` physics and render-once + GPU source-row offset
   presentation for every window. The per-layer offset generalizes: `scrollable: bool` on
   `CompositeLayer` becomes `scroll_id: u32` + `OP_SET_LAYER_SCROLL(scroll_id, src_row)`; gpud
   keeps a small id-indexed table. Search migrates off per-row CPU re-render; the chat
   band-scratch ghosting bug (scratch reused without clearing) is fixed.

## Non-Goals

- Keyboard shortcuts for window management (rejected).
- Kernel changes.
- Live font switching in the settings panel (follow-up; the key shape ships).
- A GPU glyph command (RFC-0067 "G-text" stays a separate track; CPU glyph rasterization into
  atlas surfaces is the contract here).
- Settings service/panel/theme switching — that is `TASK-0072` (same track, S-phases).

## Constraints / invariants (hard requirements)

- No company/product names in code/comments/docs/identifiers; describe patterns generically.
- Deterministic move/resize/snap math given an input sequence; pure logic host-tested
  (`frame.rs` / `window_scene.rs` / new `snap.rs`, `dock.rs` modules).
- windowd keeps focus/hit-test authority; draw order and hit order share one SSOT.
- Every visual feature ships as composite layers (renders on both the GL and CPU backends).
- Respect min sizes; clamp to display bounds; atlas exhaustion = deny with marker, never a wedge.
- No `unwrap`/`expect`; no per-frame/per-event heap allocations (bump allocators never free);
  no blanket `allow(dead_code)`.
- Honest markers only; each phase extends the headless expected-log by its own markers.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- `window_scene`: raise/focus/composition-order/hit-priority (= reverse order) tests.
- `frame.rs`: button rects (– □ ×), edge hit zones, resize clamps, snap-target frames
  (left/right halves, top = fullscreen) as pure-function tables.
- `dock.rs`: slot layout, restore mapping, visibility rule (≥1 minimized).
- SVG crate: abutting same-color shapes leave no seam (interior pixel = full color);
  translucent overlap correctness; icon golden checksums.
- Text: advance/kerning/clipping measurement tests.
- Wire goldens: `OP_SET_CURSOR_SHAPE`, `scroll_id` layer word, `OP_SET_LAYER_SCROLL`.

### Proof (OS/QEMU) — gated

UART markers (order tolerant):

- `windowd: focus id=.. z=..` / `windowd: raise id=..`
- `windowd: minimize id=..` / `windowd: restore id=..` / `windowd: dock show n=..`
- `windowd: fullscreen id=..` / `windowd: unfullscreen id=..`
- `windowd: snap edge=<left|right|top> id=..`
- `windowd: resize id=.. w=.. h=..` / `cursor: shape=<name>`
- `gpud: rt backdrop dst ok` (first destination-so-far snapshot+blur submitted)
- `windowd: font family=.. sizes=..`
- `gpud: layer scroll id=.. row=..`

### Visual proof — required (user gtk boot per phase)

- Clicking the lower window raises it; topbar sits above windows; fullscreen covers chrome.
- Minimize → dock appears bottom-center; icon click restores.
- Drag to right/left edge snaps halves; to top = fullscreen; edge hover shows resize cursors;
  corner drag resizes.
- Dragging a window across another shows the underlying window through glass + corners.
- Icons seam-free; chat/search text in the vendored UI face; both windows share identical
  inertial scroll feel with no ghosting.

## Touched paths (allowlist)

- `source/services/windowd/` (runtime scene/input, shell_window, window_scene, new
  `compositor/{snap,dock}.rs`, new `src/text.rs`, build.rs glyph/cursor bakes)
- `userspace/ui/widgets/window/` (Frame buttons/edges/resize)
- `userspace/ui/svg/` (coverage accumulation)
- `userspace/ui/widgets/virtual_list/`, `userspace/ui/layout/` (gaps only)
- `userspace/nexus-gfx/` + `source/libs/nexus-display-proto/` (scroll_id, cursor shape op)
- `source/drivers/gpud/` (cursor sprite table, RT backdrop pass, layer scroll table)
- `tasks/`, `docs/dev/ui/patterns/wm-resize-move.md`

## Plan (boot-gated phases; user boots + commits each)

1. Window collection + z/focus/raise + chrome-above-windows + wheel routing
2. Header buttons + minimize dock + fullscreen
3. Drag-to-edge snap + edge resize + cursor shapes
4. Backdrop correctness (destination-so-far; GL gaussian backdrop pass)
5. SVG conflation fix
6. Runtime text (A8 glyph atlas)
7. Scroll unification (`scroll_id` + layout→List→VirtualList everywhere)

Risks tracked in the plan: atlas budget (hard `MAX_WINDOWS`, boot-time budget marker), GL
RT-region-copy spike (fallback: fixed-order legacy path), input-routing regression (pure
host-tested hit-test), backdrop cache thrash (generation keys, measure first).
