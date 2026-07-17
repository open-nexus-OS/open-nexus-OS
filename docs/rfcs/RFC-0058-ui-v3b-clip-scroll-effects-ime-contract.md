# RFC-0058: UI v3b clipping/scroll/effects + IME/text-input contract seed

- Status: Done
- Last Updated: 2026-05-22 (compositor module refactored: `os_lite.rs` → `compositor/`) (Compositor Phases 1–6a implemented, P0/P1 closed, ShadowCache wired)
- Owners: @ui
- Created: 2026-05-17
- Links:
  - Tasks: `tasks/TASK-0059-ui-v3b-clip-scroll-effects-ime-textinput.md` (execution + proof)
  - Depends on: `docs/rfcs/RFC-0057-ui-v3a-layout-engine-pretext-contract.md` (layout engine)
  - Follow-up: `tasks/TASK-0060B-ui-v4b-glass-materials-backdrop-cache-degrade.md`
  - Layout contract: `docs/rfcs/RFC-0057-ui-v3a-layout-engine-pretext-contract.md`
  - Layout pipeline: `docs/dev/ui/foundations/layout/layout-pipeline.md`
  - Scroll spec: `docs/dev/ui/foundations/layout/scroll.md`
  - Architecture: `docs/architecture/graphics/display-output-service-chain.md`

## Status at a Glance

- **Phase 0 (Clip + scroll)**: ✅ — scissor clipping, scroll damage math, scrollbar affordance
- **Phase 1 (Text input + filter-box)**: ✅ — TextInputNode, filter_words(), filter-box proof element
- **Phase 2 (CPU effects)**: ✅ — blur/shadow, budgets, cursor blink timer
- **Phase 3 (IME/text-input stub)**: ✅ — focus routing, caret/selection, imed stub
- **Phase 4 (Host tests)**: ✅ — JSON + PNG goldens
- **Phase 5 (OS markers + postflight)**: ⬜ — QEMU markers defined, wiring pending

## Scope boundaries

- **This RFC owns**: scissor clipping via `Overflow::Hidden`, scroll damage math (viewport delta → dirty rects), scrollbar affordance (thumb + track), CPU blur/drop-shadow with budgets, TextInputNode type, keyboard → text routing, filter-box proof element, cursor blink via effect timer, IME focus-routing stub
- **This RFC does NOT own**: full IME engine (TASK-0146/0147), keymaps/OSK, clipboard, GPU effects, kernel changes

## Context

TASK-0058 delivered a deterministic layout engine with `LayoutResult { boxes, content_height }`.
v3b builds on this: scroll operates on the stable layout tree (place-only invalidation), clip
rects are derived from layout box coordinates, and effects add visual polish within budgets.

The **filter-box** is the integration test: one proof element that exercises clip (Overflow::Hidden),
scroll (wheel/drag → viewport), IME (keyboard → text input), and effects (cursor blink).

## Goals

1. Scissor clipping via `Overflow::Hidden` on layout containers
2. Scroll damage math: viewport delta → dirty rect set, place-only invalidation
3. Scrollbar affordance: visible thumb + track with hover/active states
4. CPU blur + drop shadow with deterministic budgets and degrade
5. `TextInputNode` type + keyboard → text routing
6. `filter_words(prefix)` pure function for real-time word filtering
7. Filter-box proof element integrating goals 1+2+5+6
8. Cursor blink via effect timer
9. IME/text-input stub: focus routing + caret/selection helpers
10. Host tests + OS markers

## Non-Goals

Full IME engine; keymaps/compose tables; OSK overlay; clipboard; GPU effects; text reshaping during scroll.

## Constraints

- Deterministic damage math (integer-only, order-agnostic rect comparison)
- Scroll = place-only: no text reshaping or layout remeasurement on scroll
- Bump-allocator safety: layout computation only in `new()`, scroll damage allocation-free
- Effect budgets: cap blur radius/area per frame, LRU eviction for cached effects
- No `unwrap/expect`

## Proposed design

### Clip + scroll

``` text
layout_box.overflow == Hidden → scissor rect = layout_box.rect
scroll_offset = (dx, dy) → viewport = layout_box.rect + scroll_offset
scroll_delta → dirty_rects = old_viewport ∪ new_viewport
```

Clip rects ARE layout boxes — no separate clip tree. When a container has `Overflow::Hidden`,
all children are clipped to its content rect.

### Filter-box proof element

``` text
Layout tree:
  Stack(Row) [filter_box_row]
  ├── Stack(Column) [cards_left]  ← existing hover/click/key cards
  └── Stack(Column) [filter_box_right]
      ├── Stack(Row) [filter_input]  ← TextInput + label
      │   └── TextInput { content, cursor_pos, max_length }
      └── Stack(Column, overflow: Hidden) [filter_list]
          ├── Text("apple")
          ├── Text("application")
          └── Text("apt")
```

`filter_words(prefix: &str) -> Vec<&str>` filters a static word list:
```rust
const FILTER_WORDS: &[&str] = &[
    "apple", "application", "apt", "arrow", "asset",
    "batch", "binary", "block", "buffer", "build",
    "cache", "clock", "compile", "component", "config",
];
fn filter_words(prefix: &str) -> Vec<&str> { ... }
```

### Invalidation matrix

| Change | Class | Work |
|--------|-------|------|
| scroll offset | `place-only` | reclip, reposition scrollbar |
| filter text change | `measure+place` | redo filter + list layout |
| cursor blink tick | `paint-only` | repaint cursor area |
| theme change | `paint-only` | repaint |

## Security

- IME focus scoped to focused surface only; policy can deny IME
- Effect budgets prevent memory exhaustion
- Scroll damage math bounded (no unbounded dirty region)
- No heap allocation in input hot-path

## Proof

### Host
```bash
cargo test -p ui_v3b_host -- --nocapture
```

### OS/QEMU
```bash
RUN_UNTIL_MARKER=1 just test-os visible-bootstrap
```

Markers: `windowd: clipping on`, `windowd: scroll on`, `windowd: live scroll ok`,
`windowd: text input on`, `windowd: filter list ok`, `windowd: effects on`,
`windowd: effect blur ok`, `imed: ready`, `SELFTEST: ui v3 scroll ok`,
`SELFTEST: ui v3 ime ok`, `SELFTEST: ui v3 effect ok`, `SELFTEST: ui v3 filter ok`

---

## Implementation Checklist

- [x] **Phase 0 (Clip + scroll)**: `Overflow::Hidden` → scissor, scroll damage math, scrollbar — proof: `cargo test -p ui_v3b_host`
- [x] **Phase 1 (Text input + filter-box)**: `TextInputNode`, `filter_words()`, filter-box layout, keyboard routing — proof: `cargo test -p ui_v3b_host`
- [x] **Phase 2 (CPU effects)**: blur/shadow + budgets, cursor blink — proof: `cargo test -p ui_v3b_host`
- [x] **Phase 3 (IME stub)**: focus routing, caret/selection helpers, imed stub — proof: `cargo test -p ui_v3b_host`
- [x] **Phase 4 (Host tests)**: JSON + PNG goldens — proof: `cargo test -p ui_v3b_host`
- [ ] **Phase 5 (OS markers)**: QEMU markers wired + postflight — proof: `RUN_UNTIL_MARKER=1 just test-os visible-bootstrap`

- [x] **Phase 6a (Separable blur + shadow properties + two-pass renderer)**: `blur_separable`, `blur_1d`, `BoxShadow`, `TextShadow`, `ShadowLevel`, `VisualStyle` extensions, zero-copy shadow-pass in `os_lite.rs` — proof: `cargo test -p ui_v4_host` (21 tests)

- [x] **Phase 6b (MSDF atlas)**: 32×32 SDF per glyph, 1024×96 BGRA atlas (95 ASCII glyphs), bilinear sampler, smoothstep — proof: `cargo test -p ui_v4_host` (22 tests)
- [x] **Phase 6c (SDF shapes)**: `sd_circle`, `sd_rounded_rect`, `sd_triangle`, `smoothstep`, `fill_alpha`/`border_alpha`; wired into `os_lite.rs` for anti-aliased circles and rounded rects — proof: `cargo test -p ui_v4_host` (23 tests)
- [x] **Phase 6d (9-slice shadow)**: `NineSliceShadow`, `composite_nine_slice_shadow()`, corner blur + edge stretch + center fill, `EffectCache` integration — proof: `cargo test -p ui_v4_host` (8 tests)
- [x] **Phase 6e (Dual-kawase blur)**: `dual_kawase_blur()` with downscale 2×, stride-based 3×3 blur iterations, bilinear upscale; `stride_blur_3x3` with configurable sample step — proof: `cargo test -p ui_v4_host` (7 tests)
- [x] **Phase 6f (Render cache + damage integration)**: `ShadowCache` (256 LRU), `TextCache` (512 LRU), `RenderCache` aggregator with `invalidate_dirty`/`note_scroll`/`clear` — proof: `cargo test -p ui_v4_host` (15 tests)

---

## Phase 6: NeX UI Rendering Pipeline

### Architecture Overview

``` textS
┌─────────────────────────────────────────────────┐
│                  RENDER CACHE                    │
│  node_id + params → cached layer (BGRA)         │
│  dirty flag pro entry                           │
├─────────────────────────────────────────────────┤
│  DAMAGE TRACKING                                │
│  dirty rects → invalidate cache entries         │
│  scroll → reposition, kein re-render            │
├──────────────┬──────────────┬───────────────────┤
│  MSDF ATLAS  │  SDF SHAPES  │  9-SLICE SHADOW   │
│  text+icons  │  rects,btn   │  box-shadow       │
│  scale-agno  │  analytical  │  corners+edges    │
├──────────────┴──────────────┴───────────────────┤
│  DUAL KAWASE BLUR                               │
│  downscale → iter blur → upscale                │
│  O(n·log r) statt O(n·r²)                      │
├─────────────────────────────────────────────────┤
│  SCANLINE COMPOSITOR                            │
│  layers stack → single-pass blend               │
└─────────────────────────────────────────────────┘
```

### Sub-Phase 6a: Separable Blur + Shadow Properties

Separable Blur: O(w·h·2r) statt O(w·h·r²). Horizontal-Pass + Vertikal-Pass = 30 statt 225 Ops/Pixel bei 15px.

```rust
struct VisualStyle {
    background: Option<Rgba8>,
    border: EdgeBorder,
    shadow: Option<BoxShadow>,       // ← neu
    text_shadow: Option<TextShadow>, // ← neu
    opacity: Option<Fraction>,       // ← neu
}

struct BoxShadow { offset_x: FxPx, offset_y: FxPx, blur_radius: FxPx, spread: FxPx, color: Rgba8 }
struct TextShadow { offset_x: FxPx, offset_y: FxPx, blur_radius: FxPx, color: Rgba8 }
```

Tailwind-Presets: `ShadowLevel { Sm, Md, Lg, Xl, Xxl2 }` → `to_box_shadow()`.

Renderer: Two-pass — Shadow-Layer (alpha mask + offset + spread + blur) → Content-Layer (background/border/text composited on top).

### Sub-Phase 6b: MSDF-Atlas (Text + Icons)

Build-Time: Font → SDF per glyph (32×32) → atlas packer → 1024×1024 BGRA. Runtime: `sample_atlas(glyph, uv)` → `smoothstep(0.5-a, 0.5+a, sd)` → pixel. One atlas for all sizes, shared across text, icons, emoji.

### Sub-Phase 6c: SDF-Shapes (Rounded Rects, Buttons, Panels)

Analytical: `sd_rounded_rect(p, rect, r)`, `sd_circle(p, c, r)`, `sd_triangle(p, a, b, c)`. Border: `smoothstep(border-a, border+a, sd)`. Background: `smoothstep(-a, +a, -sd)`. All parameters (corner radius, border width), no bitmaps needed.

### Sub-Phase 6d: 9-Slice-Shadow

``` text
┌──────┬──────────┬──────┐
│Corner│  Edge    │Corner│  ← 4 corners: 2D blur
├──────┼──────────┼──────┤
│Edge  │   Fill   │Edge  │  ← 4 edges: 1D stretch
├──────┼──────────┼──────┤
│Corner│  Edge    │Corner│  ← center: solid fill
└──────┴──────────┴──────┘
```

~90% fewer blur ops than full-surface blur. On resize: corners from cache, edges stretched. Cached per `(node_id, blur_radius, spread, color)`.

### Sub-Phase 6e: Dual-Kawase-Blur

Downscale 4× → 3× 3×3 blur with increasing kernel stride → upscale. ~27 samples/pixel instead of 225. Scales with log(radius).

### Sub-Phase 6f: Render-Cache + Damage-Integration

ShadowCache (256 entries, LRU) + TextCache (512 entries, LRU). Damage: dirty rect → `cache.invalidate(node_id)`. Scroll: reposition, no invalidate. Theme change: `cache.clear()`.

### Phase 7: Compositor Retained-Mode Upgrades (2026-05-20/21)

Status: ✅ **Phases 1–6a implemented, P0/P1 closed.**  
Owner: @ui  
Deliverable: `source/services/windowd/src/os_lite.rs` (~300 lines net change), `live_runtime.rs` (+9 filter rects), `tests/damage_pipeline.rs` (new).

**Phase 1: TileMap in Render-Loop** ✅
- `TileMap::has_dirty_in_row_range()` gates 4-row band writes in `write_rows`.
- `flush_pending_damage` clears tile map AFTER writes (was before → dirty tiles never seen).
- `write_current_frame` marks all 260 tiles dirty for first full-screen render.
- `queue_cursor_damage` marks tile map before calling `write_damage_rect`.

**Phase 2: LayerCache from Stub to Functional** ✅
- `LayerCache::insert()` / `get()` / `get_mut()` / `invalidate()` / `mark_clean()` added.
- `record_layer_cache_row()` populates cache row-by-row; `rows_filled` counter; `dirty = false` when complete.
- `draw_layout_box_row` checks cache before rendering; blits clean layers.
- Budget: `LAYER_CACHE_MAX_BYTES` = 4KB total, `LAYER_CACHE_MAX_LAYER_BYTES` = 1KB/layer.

**Phase 3: Backdrop Blur via Library (reverted to zero-alloc inline)** ✅
- Switched `blur_backdrop_segment` to `nexus_effects::blur_1d`.
- Reverted: library allocates `Vec` internally → bump-allocator exhaustion (`alloc-fail` after ~360 rows).
- Restored zero-allocation `blur_backdrop_segment` + `blur_row_horizontal`.
- Lesson: All hot-path functions must be zero-allocation on the OS bump allocator.

**Phase 4: Cursor-BG Save/Restore** ✅
- `cursor_bg_saved` field was allocated (4KB) but never used → now active.
- `save_cursor_bg_inline()` saves wallpaper pixels before cursor blend.
- `restore_cursor_bg()` writes saved pixels back via `vmo_write` on cursor move.
- Wired into `copy_scene_row` (save) and `apply_input_state` (restore).

**Phase 5: Paint-Only Fast-Path** ✅
- `paint_only` flag threaded through `draw_proof_surface_row` → `draw_layout_box_row`.
- Non-paint boxes (wallpaper panels, static content) skipped when `paint_only=true`.
- `combined_panels` (glass panel) excluded from skip — must always render (translucent background).
- Backdrop blur disabled for paint-only (was `!paint_only && opacity < 255`, now `opacity < 255` always).

**Phase 6a: Shadow Blur via Library (reverted) + ShadowCache** ✅
- Switched `blur_row_horizontal` in `compute_shadow_row` to `blur_1d` — reverted (same alloc-fail issue).
- Restored zero-allocation `blur_row_horizontal`.
- **ShadowCache wired**: `ShadowCache` (256-entry LRU) from `nexus-effects` imported.
- Per-row cache key: `(box_id_hash << 32) | (rel_y << 16) | blur_radius`.
- Cache check before shadow render → hit: blit cached blurred alpha, tint, composite. Miss: render, blur, store.
- Tint applied on cache-hit → color changes without cache invalidation.
- `copy_scene_row` signature extended with `shadow_cache: &mut ShadowCache`.

**Rect-Based Damage Migration (P0/P1)** ✅
- `queue_target_damage`: row ranges removed; uses `queue_dirty_rect` with precise `DamageRect` from `LayoutHotPathIndex::target_rect()`.
- `queue_cursor_damage`: `write_rows` (full rows) → `write_damage_rect` (cursor rect only).
- `flush_pending_damage`: collects rects from `pending_damage_rects` + `pending_damage_rect` → renders each with `write_damage_rect`.
- Filter/scroll damage: `queue_hot_path_rows` (row ranges) → `queue_dirty_rect` with new `filter_panel/list/input_rect` from `LayoutHotPathIndex`.
- `write_damage_rect` now accepts `glass_quality` and `paint_only` (was hardcoded `Opaque, true`).
- Shadow and backdrop blur no longer skipped for paint-only (was the shadow-overwrite bug).
- `vmo_write` batching: `write_damage_rect` renders in 4-row bands instead of per-row writes.

**Deleted Code (~150 lines)**
- `pending_damage_rows`, `MAX_DAMAGE_RANGES`, `queue_rows`, `queue_hot_path_rows`, `queue_hot_path_rect`, `queue_dirty_rect_from_rows`
- `scroll_damage_rows`, `merge_optional_ranges`, `target_state_bits`
- `fill_circle_row`, `stroke_circle_row`, `stroke_row_rect`

## Production-Grade Delta: What Remains for Production-Level Performance

**P0 — Mouse Flicker (Cursor Update Atomicity)**
- Symptom: 3-frame flicker on cursor move (restore → old rect → new rect).
- Root cause: No atomic cursor update. `fbdevd` polls between `vmo_write` calls.
- Fix: Cursor as hardware cursor (fbdevd/kernel) OR both cursor rects in single `vmo_write` batch (scatter-gather) OR double-buffer + flip.

**P0 — vmo_write per Row within a Rect**
- Symptom: Visible line-by-line update within a damage rect (30 rows = 30 `vmo_write` calls).
- Current: 4-row bands in `write_damage_rect`, but still per-row `vmo_write`.
- Fix: Single `vmo_write` per rect. Needs scatter-gather or larger `band_scratch`.

**P1 — Shadow per Box (not per Row)**
- Symptom: 60 blur ops per box (one per row). Cache entries per row → 60× LRU traffic.
- Fix: Offscreen render full box shadow once → `blur_separable` → one cache entry → per-row blit only.
- Gain: 60× fewer blur ops per box. Better for motion.

**P1 — Backdrop as Cached Layer (not 2-row LRU)**
- Symptom: 440 blur ops per frame (one per panel row). 2-row LRU cache only.
- Fix: Capture wallpaper behind panel → `blur_separable` once → store as `backdrop_layer` in `LayerCache` → per-row blit.
- Gain: 440× fewer blur ops. Needs ~1MB buffer or downscaling.

**P2 — Zero-Alloc Separable 2D Blur on OS**
- Symptom: Blur is 1D horizontal only (inline). `nexus-effects::blur_separable` allocates → unusable.
- Fix: Port separable blur to zero-allocation (pre-allocated row + column buffers).
- Gain: True 2D blur quality matching production compositors.

**P2 — Double-Buffer / VSync**
- Symptom: `fbdevd` polls VMO mid-write → partial frames visible.
- Fix: Back-buffer render → flip → `fbdevd` scans front. Needs fbdevd + kernel VMO signaling.
- Gain: No tearing, no partial updates visible.

### QEMU Evidence (2026-05-21)

`QEMU_DISPLAY_BACKEND=none RUN_UNTIL_MARKER=1 bash scripts/qemu-test.sh --profile full`:
- `windowd: ready (w=1280, h=800, hz=120)`, `SELFTEST: ui launcher present ok`, `SELFTEST: end`
- `verify-uart: profile=full clean`
- Build: 0 errors, 8 warnings (API methods intentionally kept)

### Test Status

| Suite | Count | Result |
|-------|-------|--------|
| windowd lib | 22 | ✅ |
| headless | 9 | ✅ |
| damage_pipeline | 2 | ✅ (budget gate + paint-only preservation) |
| OS check (RISC-V) | — | ✅ 0 errors |
| QEMU full | — | ✅ `SELFTEST: end` |