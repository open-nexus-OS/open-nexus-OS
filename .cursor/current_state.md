# Current State — Open Nexus OS

Last updated: 2026-05-19 (TASK-0059 Phase 6a implemented — separable blur + shadow properties)

## Active task

TASK-0059: UI v3b clip/scroll/effects + IME stub + filter-box. — **In Progress**
Status: Phases 0-5 + 6a implemented (103 tests, OS markers, zero-copy two-pass renderer).
RFC-0058: Phases 0-5 + 6a checked.
Depends on: TASK-0058 (DONE).

## What TASK-0059 Phase 6a delivered

### Separable blur (`nexus-effects`)
- `blur_1d(pixels, width, height, stride, radius, horizontal) -> u32`: sliding-window O(w·h) per pass
- `blur_separable(pixels, width, height, stride, radius) -> u32`: horizontal + vertical = 2D box blur
- Zero-copy: row buffer reused per row in horizontal pass; single transpose buffer for vertical pass

### Shadow types (`nexus-layout-types`)
- `BoxShadow { offset_x, offset_y, blur_radius, spread, color }`: element shadow descriptor
- `TextShadow { offset_x, offset_y, blur_radius, color }`: text shadow descriptor
- `ShadowLevel { Sm, Md, Lg, Xl, Xxl2 }`: Tailwind presets → `to_box_shadow()`
- `Fraction` extended with `OPAQUE`, `TRANSPARENT`, `as_u8()`, `blend_factor()`
- `VisualStyle` extended: `shadow: Option<BoxShadow>`, `text_shadow: Option<TextShadow>`, `opacity` now `Option<Fraction>`

### Two-pass renderer (`windowd/os_lite.rs`)
- Zero-copy architecture: `shadow_scratch` + `blur_row_buf` pre-allocated at startup (no per-row/frame allocs)
- `compute_shadow_row()`: per-row shadow pass — alpha mask → horizontal blur → tint → over-composite
- Shadow pass runs between wallpaper and content in `copy_scene_row()`
- `blur_row_horizontal()`: inline zero-allocation single-row blur using sliding window
- `windowd/Cargo.toml` now depends on `nexus-effects`

### Tests (`tests/ui_v4_host/`)
- 21 tests: `blur_separable` (2), `blur_1d` (2), `BoxShadow` (1), `TextShadow` (1), `ShadowLevel` (6), `VisualStyle` (5), `Fraction` (4)

## Files changed/created

### New files
- `tests/ui_v4_host/Cargo.toml`, `src/lib.rs` (21 tests)

### Modified files
- `userspace/ui/effects/src/blur.rs`: `blur_1d`, `blur_separable`
- `userspace/ui/effects/src/lib.rs`: re-exports
- `userspace/ui/layout-types/src/border.rs`: `BoxShadow`, `TextShadow`, `ShadowLevel`, `VisualStyle` extensions, `Fraction` imports
- `userspace/ui/layout-types/src/node.rs`: `Fraction` methods (OPAQUE, TRANSPARENT, as_u8, blend_factor)
- `userspace/ui/layout-types/src/lib.rs`: new exports
- `source/services/windowd/Cargo.toml`: `nexus-effects` dep
- `source/services/windowd/src/os_lite.rs`: `shadow_scratch`, `blur_row_buf`, `compute_shadow_row()`, `blur_row_horizontal()`, two-pass `copy_scene_row()`
- `Cargo.toml`: workspace member `tests/ui_v4_host`
- `CHANGELOG.md`: TASK-0059 Phase 6a entry
- `docs/rfcs/RFC-0058-*.md`: checklist updated

## Proofs

```bash
cargo test -p nexus-layout       # 9/9
cargo test -p nexus-layout-types # (0 doc tests)
cargo test -p nexus-effects      # (0 doc tests)
cargo test -p windowd            # 31/31
cargo test -p imed               # 6/6
cargo test -p ui_v3a_host        # 13/13
cargo test -p ui_v3b_host        # 20/20
cargo test -p ui_v4_host         # 21/21
just dep-gate                    # PASS
```

## Pending

### Phase 6b–6f (NeX UI Rendering Pipeline)
- **6b**: MSDF atlas (`userspace/ui/msdf/`) — text + icons, scale-agnostic
- **6c**: SDF shapes (`userspace/ui/sdf/`) — rounded rects, circles, panels
- **6d**: 9-slice shadow in effects — corner blur + edge stretch
- **6e**: Dual-kawase blur in effects — log-scaling
- **6f**: Render cache + damage integration — ShadowCache, TextCache

## Architecture decisions

| Decision | Rationale |
|----------|-----------|
| Zero-copy shadow pass | `shadow_scratch` + `blur_row_buf` allocated once at startup; no per-frame or per-row heap allocs |
| Horizontal-only blur in per-row pass | Separable blur: horizontal pass is row-local; vertical pass implicit in scanline order |
| `Fraction` for opacity | Reuses existing `Fraction(u32)` type (0-255 range) from grid fractions |
| `ShadowLevel` enum with const `to_box_shadow()` | Tailwind-style presets; deterministic mapping to `BoxShadow`, no runtime lookup |
| Pre-allocated blur row buffer | Bump-allocator safety: `blur_row_buf` reused every row, never reallocated |
