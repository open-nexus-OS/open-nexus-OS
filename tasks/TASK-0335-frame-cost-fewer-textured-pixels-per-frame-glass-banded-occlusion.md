---
title: TASK-0335 The frame cost — fewer textured pixels per frame (the glass re-blurred in its changed band, occluded surfaces not drawn, the scene under glass drawn once), judged by RFC-0108's frame line
status: In Progress (P4's first cuts board-proven in cycle 62; the rest — the band's clean, the blur's dual filter — parked to layer 8's closing by the operator 2026-10-22)
owner: @gpu @ui
created: 2026-10-20
depends-on: [TASK-0334 (RFC-0108 Phase 0 ✅), TASK-0215 (5d ✅)]
follow-up-tasks: [TASK-0336 the window as one surface (first, after the P3 commit), TASK-0337 one window path + materials (the opaque pane P2 occludes with), then P2 + the programs here, the non-parking compositor (ADR-0075), 6c the launch without a copy]
links:
  - RFC: docs/rfcs/RFC-0108-ux-latency-contract-instant-and-smooth-as-numbers.md (the frame budget this meets)
  - RFC: docs/rfcs/RFC-0101-gpu-driver-split-contexts-vm-batches-fence-and-the-scanout-contract.md (§8 the planner, damage, the glass cache)
  - Measurement: docs/board/measurements/2026-10-20-memory-lifecycle-baseline/README.md (cycle 49: the cost model)
  - Planner: userspace/nexus-gfx/src/frame/{plan.rs, board.rs, glass.rs}; executor: source/drivers/gpud/src/backend/gpu/{desktop.rs, desktop_pass.rs, desktop_work.rs}
---

## Context

- The board feels like a slideshow (cycle 47) and RFC-0108's line says why: `gpud: ux frame FAIL`
  in every window, 14–32 of 32 frames past 16.7 ms, `gpu frame us` 44–84 ms with 95 % in the
  GPU's `wait`; windowd presents 8 frames a second (`windowd: loop … present=8`) and an input
  waits 0.6–5 s for the frame that carries it (`windowd: ux input FAIL`).
- Read from cycle 49's windows: the GPU's wait is ~6.5 ms **per pass** across windows of 0.8–1.6
  Mpx and 4–13 passes (the two 18 ms-a-pass windows carry the largest glass scenes); the boot
  trace shows a full-screen pass at 12 ms and the raw fill at 1 ns a pixel. The GPU is
  memory-bound at a few nanoseconds per **textured** pixel: a frame's cost follows the pixels its
  passes draw (the slot's area × its layers, each glass re-blur's scene below it over its reach,
  the blurs) — 5–10 Mpx a frame today, where 16.7 ms buys ~3 Mpx.
- The planner already re-blurs a glass only when its backdrop changed (`glass.rs`, cycle 38/41);
  what changes under the shell's glass every pointer move is a window's content, and the
  Control Center's full-screen glass is touched by any damage.
- Must not change: the planner's invariants (RFC-0101 §8: damage per ring slot, the glass cache's
  rules), the wire of the command buffer unless RFC-0101 names the extension, the kernel.

## Goal

`gpud: ux frame ok` on the board with the operator's series — the desktop, a window hovered,
the Control Center open — and the input's wait with it (the GPU's backlog gone).

## Non-Goals

- A faster GPU clock or a different blur kernel for its own sake (the count is the lever).
- The compositor's delivery contract (ADR-0075, the next package); the app's first frame (6c).

## Three lenses (architecture-review)

- **Scope**: the planner (`nexus_gfx::frame`, pure, host-tested) and the executor's lines
  (`gpud`); the command buffer's layer only if an `opaque` hint is needed (RFC-0101 §8 extension,
  its wire test). Out: windowd's scene (it sends what it sends), the kernel, the display ring.
- **Invariant**: every cut draws the same pixels the full frame would have (host: a golden of the
  final slot pixels per scene equals the uncut plan's — `test_reject_a_cut_that_changes_a_pixel`);
  the cache rows outside a re-blurred band keep their content.
- **Contract**: RFC-0101 §8 (the planner's damage and glass rules), RFC-0108 (the budget).

## Plan (each step measured by `gpud: gpu work` and `gpud: ux frame`)

- **P0 — the count.** `Board::work_px(pass)` (host-tested) and
  `gpud: gpu work (slot_px= scene_px= blur_px= ns_per_px=)` per 32-frame window: the textured
  pixels by pass kind and the GPU's wait per pixel. Cycle 50 confirms the model (ns_per_px
  roughly constant across windows) and says which kind dominates.
- **P1 — the glass in its changed band.** The planner carries the damage that touched a glass
  (the union of the touching rects, not a bool); a `Write` re-blurs only that band grown by the
  blur's reach: the scene pass over the band, the horizontal pass over its half-resolution
  columns, the vertical pass over its cache rows; the rest of the rows keep. Host: the band's
  geometry; the golden invariant.
- **P2 — occluded surfaces not drawn.** Walking front to back, an opaque layer's rectangle
  (opacity 255, inset by its corner radius) is subtracted from the draws below it (one occluder a
  draw, ≤ 4 pieces: bounded, deterministic); needs the layer to say it is opaque — the
  `opaque` hint in `CompositeLayer` (RFC-0101 §8 extension, the wire test pinned), windowd sets
  it for app windows whose surface is opaque and for the wallpaper.
- **P3 — the scene under glass drawn once.** Several glasses re-blurred in one frame share one
  scratch: the scene below is drawn once up to the highest, each glass's reach taken from it.
- **P4 — the programs** (handed over from TASK-0342 steps 4–6, 2026-10-22: since the frame chain
  holds, the GPU's frame is the pace — cycle 61: 25 ms median, 33 ms while typing, 13 ns a pixel
  on the median against the vendor compositor's 1.9–3.7, frames of three or more passes 33.6 ms
  against 18.2 at one; `docs/board/measurements/2026-10-22-frame-chain/`): a layer's interior a
  plain copy and the SDF at its rounded edges alone (where the corner squares do not already cut
  it), the shadow shaded only outside the box that knocks it out (a ring), the blur's dual filter
  (a down/up chain at half and quarter resolution in place of the separable gaussian) — each
  priced first by `gpud: gpu work` against the operator's series.
- **Proof**: host tests per step; `just test-os visible`; board cycles with the operator's
  series, the `ux frame` verdict and `gpu work` read into the measurement folder; the package
  closes when `gpud: ux frame FAIL` leaves `config/fail-marker-allow-board.txt`.

## Results

- **P0 built 2026-10-20** (cycle 50 pending): `Board::work_px`, `desktop_work.rs`, the line.
- **P1 built 2026-10-20** (cycle 50 pending): the planner carries the damage that touched a
  glass as a band (`Glass::band`, `Area::intersect`, `GlassCache::would_hit`): a write whose
  rows already hold this backdrop blurs the band alone — the scene below over the band's
  reach, the horizontal pass over its half-resolution columns and the rows the vertical taps
  read, the vertical pass over its cache rows (`Board::chain`); the rest of the rows keep. The
  frame's damage for such a write is the band's reach within the glass, not the glass. A new
  or reshuffled backdrop (the key, the rows' place) or a full present blurs the whole glass as
  before. Host: `a_change_under_a_cached_glass_names_the_band_it_blurs_again` (plan),
  `a_glass_written_for_a_band_blurs_the_band_alone` (board, the three areas), the existing
  reach test updated to the banded damage; nexus-gfx 141 + 24 + 12 tests, gpud 36. Executor
  untouched: a pass draws only inside its area, the targets persist outside it (the same rule
  the slot's damage and the cache's rows already rest on).
- **Cycle 50 (board, 2026-10-20) — P0 + P1 read:** `gpu work` names the slot pass as the
  pixels (1.7–3.6 Mpx a frame; the glass scenes 0.13–1.4 M, the blurs ≤ 0.05 M) and the GPU
  at 10–30 ns per textured pixel; glass writes 22–114 per 32 frames; the operator felt no
  difference (the frames 29–77 ms as before). P1 cut the smaller item.
- **The vendor answers (2026-10-21, `docs/board/measurements/2026-10-21-gpu-vendor-per-pixel/`):**
  a pixel costs the vendor 1.85 ns opaque, 3.18 blended, 3.72 rounded (ours 1.21 / 4.89 / 8.85:
  the shader is 1.5–2.4× of it, not 10×); a linear texture costs nothing extra (the atlas's
  layout stays); **a render costs 0.52 ms fixed** (eight renders 4.1 ms, eight draws in one
  0.9 ms). A 60 Hz frame on this GPU is two full-screen blended layers or a few renders — never
  both. **The order changes: P3 (the passes) first, then P2 (the pixels), the programs last.**
- **P3 built 2026-10-21** (cycle 51 pending): the glasses written in a frame share passes.
  `chain::chain` gives one glass's re-blur geometry at any half rows (the band of P1 kept);
  `merge::mergeable` decides, host-tested, whether the writes may share one scene pass that
  draws the layers below the lowest write over every write's reach — no non-write layer
  between the lowest write and the top, and no other write's rectangle, in any write's scene
  area — and the board packs the writes' horizontal results one after another in the half
  scratch; when they fit and may share, the frame runs **four passes** (the scene, one
  horizontal pass of every write's quad cut to its band (`chain::clip_blur`, exact), one
  vertical, the slot) instead of `1 + 3 × writes`; otherwise the chains one by one as before.
  The executor is untouched (`PassKind::Blurs` is many draws like a scene's). Host:
  `two_glasses_written_apart_share_one_scene_and_two_blur_passes`,
  `a_glass_over_another_written_glass_keeps_its_own_chain`, the merge rule's three cases, the
  clip's sampling identity; nexus-gfx 146 + 14 + 24, gpud 36.
- **The damage's source counted 2026-10-21** (cycle 51 pending): windowd's present collects its
  rects into a list of four, each joining the first it touches (a bounding box);
  `windowd: loop … damage(rects= queued_px= flushed_px=)` names the rects queued, their pixels,
  and the pixels the present sent — the ratio is the bounding-box growth the planner's slot
  pass then pays. Found on the way: a fifth rect touching none of the four was dropped (a
  stale pixel); it now joins the rect whose union grows the least
  (`windowd::damage_rects`, host-tested — the compositor's own unit tests never ran on the
  host; `test_reject_dropping_a_fifth_disjoint_damage_rect`). windowd 194 lib tests.
- **The operator's question, answered from the numbers** ("the compositor sends only the
  changes, the glass is blurred once and cached, less blur while moving"): the design holds —
  the planner draws the slot's damage only, re-blurs a glass only when its backdrop changed and
  since P1 only in the band that changed. What the board measured leaking is (1) the damage
  itself: windowd's four bounding boxes grow to the slot's 1.7–3.6 Mpx when a window's
  content and the pointer change far apart (the counters above say by how much); (2) a glass
  over a moving window re-blurs every frame because the content under it changes — a cheaper
  blur while things move (the backdrop frozen or at half the taps during motion, the full
  blur when it rests) is not built yet; (3) each render costs 0.52 ms fixed (the vendor's
  number), so a frame of 4–13 passes paid 2–7 ms before a pixel — P3; (4) layers under an
  opaque window are still drawn — P2.

- **Cycle 51 (board, 2026-10-21; `docs/board/measurements/2026-10-21-gpu-vendor-per-pixel/`
  "Cycle 51"):** 51a faulted gpud at the first present — P3's board (7.4 KiB of chains) on a
  64 KiB stack without margin; gpud declares 32 pages. 51: P3 holds where the writes may share
  (3.4–4.7 passes a frame at 1–1.5 writes; 8.5–12.8 with several windows open, the chains one
  by one), **the wait did not move with the passes** (31 ms at 4.7 passes, 74 at 12.8): the
  pixels rule — 8.3 layers a frame, 2.46 Mpx of layers + 1.25 Mpx of wallpaper blits + 0.32 Mpx
  of glass over 1.0–1.4 Mpx of damage, 3.4× overdraw at 8–20 ns a pixel. `ux frame` FAIL in
  every window (12–32 of 32). Next: P2 (the wallpaper and the layers under an opaque window not
  drawn, the opaque program), the programs, the scene shared per group. `windowd: loop` hid
  the board (needs ≥ 8 presents a second) — report every window with traffic.
- **The three window paths (cycle 51, the operator's observation):** calculator and chat run
  `style=titlebar` (windowd's legacy title bar over a plain surface; chat past the band cap
  on the plain path, whole-frame presents), settings and stash `style=plain` with window-kit's
  `WinAppWindow` on the band path. One window path first (windows-as-widgets.md), or every
  cut here is measured on one construction and missed on the other two.
- **P2 built 2026-10-21 (host-green, board cycle 52 pending):** occlusion on the board — an
  opaque layer (`LAYER_OPAQUE`, full opacity; the rectangle inset by its corner radius,
  `frame::occlude`) cuts every draw below it to the pieces around it: the wallpaper, the layers,
  their shadows (`Quad::part_of`), a glass's cached backdrop on its scale's grid; one occluder a
  draw (the largest touching), ≤ 4 pieces, deterministic (`frame::pieces` holds a layer's draws
  now, out of `board.rs`). Opaque straight parts run the **opaque program** (1.2 instead of 4.9
  ns a pixel). Host: `tests/frame_occlusion.rs` (the four bands disjoint and complete, the texels
  of a piece the part its pixels showed, the reject cases: no flag, faded, the corners).

- **P4 — the first cuts built 2026-10-22 (board cycle 62 pending, with TASK-0343; the operator:
  "when scrolling no rounded corners need to be computed, only the scroll content"):** cycle
  61's scroll frames waited 16.4–17.1 ms on the GPU for 0.95 Mpx of damage — the boot gate's
  prices (rounded 8.69 ns a pixel, shadow 10.55, straight 4.89, opaque 1.34; a frame's base
  ~3.9 ms) give 3.9 + 0.95 × (8.69 + 4.89) ≈ 16.8 ms: the glass's cached backdrop drawn with the
  rounded program over the whole viewport. Cut, each draw the same pixels as before:
  (1) a glass's back is its cross on the opaque program and its four corner squares on the
  rounded one, cut on the cache's grid (`pieces::rounded_parts`, shared with the content's
  parts: the corner squares grown to the grid, the straight parts inside the cross where the
  coverage is 1); (2) a shadow is its ring — its quad less the layer's shape, plus the shape's
  corner squares; inside the cross its knockout is zero at every pixel centre (`shadow.wgsl`);
  (3) a glass whose back is drawn hides what lies under it inside its corners — the wallpaper,
  lower windows (`occlude::opaque_rect(l, back)`); (4) the executor emits only the draws that
  touch a pass's area (the back end's clip writes no other pixel); (5) a scroll step cleans only
  the atlas rows it shows new (`desktop_pass::CleanedRows`; a new content generation forgets
  every range — which also closes a stale-range hole of the old rule); (6) `gpud: gpu work`
  counts the pixels by program (`Board::work_by_program`). Host: the parts' coverage and the
  ring's knockout checked pixel by pixel against the shaders' formulas
  (`a_rounded_rectangle_is_its_cross_and_its_corner_squares_on_any_grid`,
  `a_shadow_is_drawn_as_its_ring_and_test_reject_a_lost_shadow_pixel`),
  `a_scroll_inside_a_glass_window_computes_no_corner_and_no_shadow` (the slot's work: the glass
  back opaque over the viewport, the window's content and the scroll layer straight, nothing
  rounded, no shadow), `a_drawn_glass_back_hides_what_is_below_and_test_reject_a_glass_without_it`;
  nexus-gfx 159 + the integration suites green. The blur's dual filter stays open.
  **Board cycle 62** (`docs/board/measurements/2026-10-22-frame-pace/README.md`): in the nine
  scroll windows `rounded_px=0 shadow_px=0`, `opaque_px` ≈ the damage (the glass's back alone,
  nothing under it), the GPU's wait **6.5–8.1 ms** a frame (was 16.4–17.1), `gpud: ux frame ok
  (dropped=0 of=32)` in all nine; the operator: scrolling "great". **Open:** the CPU's clean
  stays 2.3–2.6 ms in six of the nine — the chat's band takes new strips while it scrolls, and a
  frame with an upload forgets every cleaned row; next: forget only the rows the upload wrote.
