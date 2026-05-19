# Handoff — TASK-0059 **In Progress** (Phase 6a Done)

Date: 2026-05-19

## Status

- RFC-0058: Phases 0-5 ✅, Phase 6a ✅ (separable blur + shadow properties + two-pass renderer)
- TASK-0059: Phases 0-5 + 6a implemented
- Depends on: TASK-0058 (DONE)
- Follow-up: TASK-0060B (glass materials)

## What was delivered in Phase 6a

### Separable blur (`nexus-effects/src/blur.rs`)
- `blur_1d(pixels, width, height, stride, radius, horizontal) -> u32`: sliding-window O(w·h)
- `blur_separable(pixels, width, height, stride, radius) -> u32`: 2D box blur via horizontal+vertical pass
- Zero-copy: row buffer reused per row; single transpose buffer for vertical pass

### Shadow types (`nexus-layout-types/src/border.rs`)
- `BoxShadow { offset_x, offset_y, blur_radius, spread, color }`
- `TextShadow { offset_x, offset_y, blur_radius, color }`
- `ShadowLevel { Sm, Md, Lg, Xl, Xxl2 }` → `to_box_shadow()`
- `Fraction` extended: `OPAQUE`, `TRANSPARENT`, `as_u8()`, `blend_factor()`
- `VisualStyle` extended: `shadow`, `text_shadow`, `opacity` as `Option<Fraction>`

### Two-pass renderer (`windowd/src/os_lite.rs`)
- Zero-copy: `shadow_scratch` + `blur_row_buf` pre-allocated at startup
- `compute_shadow_row()`: per-row shadow compositing (alpha mask → blur → tint → over)
- `blur_row_horizontal()`: inline zero-allocation single-row blur
- Shadow pass inserted between wallpaper and content in `copy_scene_row()`

### Tests (`tests/ui_v4_host/`)
- 21 tests: blur_separable (2), blur_1d (2), BoxShadow (1), TextShadow (1), ShadowLevel (6), VisualStyle (5), Fraction (4)

## Files changed

### New
- `tests/ui_v4_host/Cargo.toml`, `src/lib.rs`

### Modified
- `userspace/ui/effects/src/blur.rs`, `lib.rs`
- `userspace/ui/layout-types/src/border.rs`, `node.rs`, `lib.rs`
- `source/services/windowd/Cargo.toml`, `src/os_lite.rs`
- `Cargo.toml`
- `docs/rfcs/RFC-0058-*.md`

## Proof

```bash
cargo test -p nexus-layout       # 9/9
cargo test -p windowd            # 31/31
cargo test -p ui_v3a_host        # 13/13
cargo test -p ui_v3b_host        # 20/20
cargo test -p ui_v4_host         # 21/21
just dep-gate                    # PASS
```

## Next step (Phase 6b: MSDF atlas)

Create `userspace/ui/msdf/` crate:
1. Atlas packer (glyph SDFs → 1024×1024 BGRA)
2. SDF generator (font → 32×32 glyph SDF via `fontdue`)
3. Runtime sampler (`sample_atlas(glyph, uv)` → pixel)
4. Build-time atlas compilation via `build.rs`
5. `cargo test -p nexus-msdf`
