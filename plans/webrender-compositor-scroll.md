# WebRender Compositor Scroll (Item 2 / Scroll-Track "next jump")

Goal: the chat app renders its scrollable content into a TALLER-than-visible band
ONCE; the compositor (gpud) shifts only the source row (`src_row`) per scroll
frame via `OP_SET_LAYER_SCROLL`. Scroll frames become pure GPU re-composites at
display rate instead of the app re-painting + re-uploading every frame (the
current ~67Hz limit). Composes with Item 1: `tail(messages,64)` bounds the
resident content, so the band is finite (~2600px) and one-time.

## Already DONE (verified)
- gpud: `OP_SET_LAYER_SCROLL(scroll_id,src_row)` handler (service.rs:1081),
  `scroll_src_rows[]` override at composite (backend/present.rs:825-832), src_row
  UV select (virgl_composite.rs:218-219). `PendingRtLayer` has scroll_id/content_w/h.
- Wire: `Command::CompositeLayer` carries scroll_id + content_w/h (nexus-gfx buffer.rs:106).
- windowd: UNUSED builder `composite_scrollable_glass` (shell_window.rs:500).
- Item 1: resident window capped at 64 msgs (tail).

## Chosen approach: A — "full resident band" (NOT prefetch-margin)
Resident is already capped (Item 1) → band is finite + one-time. No per-scroll
band-origin coordination (B's permanent risk). Every scroll frame = pure src_row
shift; app re-renders only on LoadMore.

### KEY CORRECTION vs naive framing
- `content_h` in gpud is the SCALED source sub-size (live-resize grow path,
  present.rs:496) — NOT the scroll lever. For scroll: `content_h = 0` (1:1),
  `scroll_id != 0`, and the ATLAS BAND must be physically tall (windowd blits the
  whole tall band once). gpud transfers only `height` rows at `src_row_abs`.
- Chat has a FIXED FOOTER (composer: TextField+Button) + fixed header (Toolbar).
  So it's a 3-SLICE packed band composite (fixed top / scrolling body / fixed
  bottom), not a flag flip. This is the central new work + main regression risk.

## Steps (each phase must leave the tree BOOTABLE; content_h==0 = unchanged)

### Phase 0 — capacity
1. windowd client_surface.rs:29 — raise MAX_SURFACE_H 800 → ~3072 (scrollable cap).
2. open_app_window (app_window.rs:551) — content band alloc tall; blur band stays
   VISIBLE-sized (decouple; tall blur wastes atlas + glass clamps to w×h anyway).

### Phase 1 — protocol (additive; 0 defaults = current behavior)
3. nexus-display-proto client_surface.rs:389-434 — SURFACE_CREATE + u16 content_h,
   header_h, footer_h (0 = non-scrollable). Bump SURFACE_CREATE_FRAME_LEN +6,
   update encode/decode. (Band alloc'd at create → geometry must arrive atomically;
   resize re-sends CREATE so LoadMore growth reuses it.)
4. same file :314 — add INPUT_KIND_SCROLL_POS = 4; windowd→app pushes absolute
   scroll_y (rows) in the existing surface-input y field.

### Phase 2 — windowd: band ≠ frame, scroll ownership
5. AppWindowSlot (runtime/mod.rs:209) — add scroll_id (=slot_index+1; MAX_APP_WINDOWS
   ≤ MAX_SCROLL_IDS=8), content_h, header_h, footer_h, scroll_rows, reuse
   animation::ScrollMomentum. windowd becomes scroll OWNER (single writer).
6. handle_surface_create (app_window.rs:43) — decode 3 fields; frame height stays
   VISIBLE (height+title_h); store content_h/header_h/footer_h on slot.
7. open_app_window (app_window.rs:551) — content alloc(w, content_h.max(win.h)) tall;
   blur alloc(w, win.h) small; keep fail-closed QUOTA.
8. render_app_surface (app_window.rs:612) — blit loop iterates BAND height
   (title_h+client.height) so all tall rows land in atlas. Runs only on surface_dirty.
9. scene.rs:217-277 — if slot scrollable (scroll_id!=0) call extended
   composite_scrollable_glass with content_offset = header_h+footer_h+scroll_rows,
   header_h = title_h+slot.header_h, + footer_h. Snapshot new fields into AppSceneSnap.
10. composite_scrollable_glass (shell_window.rs:500) — emit THIRD fixed footer slice
    (band rows [atlas_row+title_h+header_h, +footer_h) → dst [win_bottom-footer_h,
    win_bottom), opaque). Body layer keeps content_w/h:0, scroll_id set.

### Phase 3 — wheel → OP_SET_LAYER_SCROLL (the payoff)
11. forward_wheel (input.rs:563) Body arm — if slot scrollable: feed notch into
    windowd ScrollMomentum, clamp max = content_h - visible_body_h, update scroll_rows,
    emit OP_SET_LAYER_SCROLL [op(mod.rs:64), scroll_id LE, (atlas_row+title_h+header_h
    +footer_h+scroll_rows) LE] via send_gpud_fire_forget (gpud.rs:211); push
    INPUT_KIND_SCROLL_POS to app (scroll_y for hit-test/EndReached, NO re-render).
12. keep fling alive: while windowd ScrollMomentum animates, advance scroll_rows per
    pacer tick + re-emit OP_SET_LAYER_SCROLL (app out of per-frame loop).

### Phase 4 — app-host: paint packed band once, stop re-painting on wheel
13. main.rs startup (:278 VMO, :299-343) — mount+layout at VISIBLE height; compute
    content_h/header_h/footer_h from scroll_region() + Toolbar/composer boxes; create
    TALL VMO (w × (header_h+footer_h+content_h)); render packed band; encode_surface_create
    with the 3 new fields.
14. render_band routine (near render_rows :1270) — scroll-content boxes (clip_rect)
    painted IDENTITY at band_content_top + (model_y - clip.1) (no dy); Toolbar → band
    [0,header_h); composer → [header_h,header_h+footer_h). Reuse paint_row_picked scroll=None.
15. wheel handler (main.rs:617) — compositor-scrolled surface: do NOT scroll_wheel/re-render.
    Handle INPUT_KIND_SCROLL_POS: set scroll_y=pushed, run EndReached near-end check +
    fire_end_reached → on LoadMore re-layout+re-render tall band+re-present. Tap uses scroll_param.

## Regression risks + guards
- Footer/scroll pixel collision (#1): footer overlay opaque, sampled from reserved band
  region app never scrolls; assert 3 slices (top/body/footer) tile the window (no
  overlap/gap) before wiring gpud.
- Atlas starvation: blur band visible-sized (Phase 0/7); fail-closed QUOTA must degrade
  to "no window" not garbage (glass clamps to atlas.height).
- scroll_y desync: windowd single writer; app mirrors pushed value, bypass app momentum
  for compositor-scrolled surface.
- stale src_row on full present: composite layer src_row_abs = atlas_row+header_h+footer_h
  +scroll_rows (same as override) so full present + scroll flush agree (no snap-to-top).
- MAX_SURFACE_H bump: content_h==0 = non-scrollable default; normal apps height≤800, no band.

## Boot-verify
1. src_row LIVE: `gpud: layer scroll live id=1 row=N` (gl_scanout.rs:470) fires only from
   record_layer_scroll (i.e. windowd emitted OP_SET_LAYER_SCROLL). + `windowd: wheel fwd`.
2. app stopped per-notch repaint: `APPHOST: interactive frame presented` fires only on
   LoadMore, not per notch.
3. no regression: fling still smooth (frame pulses), `apphost: scroll window texts=N` at end.
4. full-present agreement: move window mid-scroll → transcript stays put (no snap-to-top).

## Critical files
- windowd: compositor/runtime/app_window.rs, input.rs, compositor/shell_window.rs,
  compositor/runtime/scene.rs, client_surface.rs (MAX_SURFACE_H), runtime/mod.rs,
  compositor/runtime/gpud.rs (send_gpud_fire_forget).
- app-host: src/main.rs.
- proto: libs/nexus-display-proto/src/client_surface.rs.

## DEBUG FINDINGS (2026-07-12, boot 16-33-59) — Phases 2-4 implemented, ONE bug left

Boot-verified WORKING:
- app-host packed-band render: chat packs correctly (fixed Toolbar header / scrolling
  message body / fixed composer footer) — 3-slice band structure is CORRECT.
- windowd scroll ownership: on wheel over the chat body, windowd emits OP_SET_LAYER_SCROLL
  (marker `gpud: layer scroll live id=2 row=4176`), does NOT forward INPUT_KIND_WHEEL to
  the app (`APPHOST: wheel rx` +0), and the app does NOT re-render per notch
  (`interactive frame` +0). The WebRender wiring is LIVE and correct.

BUG (visual): the chat body does NOT visibly scroll. It shows the content TOP (#1/#2/#3)
regardless of scroll_rows (pinned at max, row=4176). Confirmed via BOTH paths:
- gpud override: `record_layer_scroll(id=2, row=4176)` fires but the composited body does
  not move → gpud's OP_SET_LAYER_SCROLL flush/composite does NOT apply scroll_src_rows to
  the RT layer. This op was "ungenutzt bereit" = NEVER driven e2e before, so the gpud path
  (gl_scanout.rs flush_layer_scroll / backend/present.rs:825 composite override) is likely
  broken/untested — flush_layer_scroll may re-scanout WITHOUT re-running
  composite_pending_rt_layers with the override (rt_layers_dirty stays false).
- scene.rs baseline: forcing a FULL present (drag the window) STILL shows the top → the
  body layer is composited with scroll_rows=0, i.e. composite_scrollable_glass's
  content_offset is NOT reflecting the updated slot.scroll_rows (AppSceneSnap.scroll_rows
  snapshot stale, or the offset param not wired through).

NEXT (focused, ~1-2 boot iterations):
1. Verify gpud actually re-composites the RT layer on OP_SET_LAYER_SCROLL (not just
   re-scanout): flush_layer_scroll must re-run composite_pending_rt_layers so the
   scroll_src_rows override reaches virgl_composite src_row UV. Add a marker inside the
   composite override branch (present.rs:825) to confirm it's hit with row=4176.
2. Verify scene.rs snapshots the CURRENT slot.scroll_rows into AppSceneSnap and
   composite_scrollable_glass passes content_offset = header_h+footer_h+scroll_rows to the
   body layer's src_row.
3. Watch the title_h off-by: content in the atlas starts at atlas_row+header_h+footer_h
   (NO title_h — the app band row 0 is the Toolbar, not the windowd title bar). The
   src_row formula currently adds title_h; drop it unless render_app_surface blits the band
   at atlas_row+title_h.
Phases 2-4 are UNCOMMITTED — `git checkout .` restores the working item-1 chat.
