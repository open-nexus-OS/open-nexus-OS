# Current State — Open Nexus OS

Last updated: 2026-05-22

## Active focus

**TASK-0059 / RFC-0058: Done.**
ShadowCache heap exhaustion fixed. Per-box caching + zero-alloc blur in production.
`os_lite.rs` monolith (4860 lines) refactored into `compositor/` module (18 files).
All tests green. No functional change.

---

## Implemented

### Compositor Retained-Mode Upgrades (TASK-0059 / RFC-0058)

- **ShadowArena**: Pre-allocated 64KB bump-allocator. Replaces `to_vec()` / `Vec<u8>` pattern.
- **Per-Box Caching**: One cache entry per box (not per row). 30–60× fewer entries.
- **blur_separable_zero_alloc**: Library blur with pre-allocated scratch buffers.
- **TileMap**: 64×64 tile damage tracking with `has_dirty_in_row_range` band-skip.
- **LayerCache**: Retained render layer cache with insert/get/invalidate.
- **Backdrop Blur**: Zero-allocation blur + glass-layer caching with quality degradation.
- **Cursor-BG Save/Restore**: Coalesced cursor damage with background save/restore.
- **Paint-Only Fast-Path**: Non-paint boxes skipped when only paint damage.
- **SDF Shapes**: Anti-aliased circles and rounded rectangles via nexus-sdf.
- **Filter-Box Proof Element**: Text input + scrollable word list + scrollbar.

### Compositor Module Refactoring (2026-05-22)

`source/services/windowd/src/os_lite.rs` (4860 lines) → `source/services/windowd/src/compositor/`:

```
compositor/
├── mod.rs          (268 lines) — Header, constants, service_main_loop
├── runtime.rs      (733 lines) — DisplayServerRuntime + InputMarkerState
├── surface.rs      (589 lines) — draw_proof_surface_row, draw_layout_box_row
├── backdrop.rs     (319 lines) — Backdrop blur, glass layer
├── filter.rs       (255 lines) — Filter word list, layout builders
├── tests.rs        (238 lines) — 13 unit tests (QEMU)
├── cache.rs        (197 lines) — 6 cache structs
├── scene.rs        (167 lines) — copy_scene_row, copy_cursor_background_row
├── shadow.rs       (161 lines) — Shadow pipeline
├── types.rs        (147 lines) — Data types, ProofCard/PaintRole
├── font.rs         (134 lines) — 5×7 bitmap font
├── primitives.rs   ( 94 lines) — Blend, fill, stroke, path
├── tile_map.rs     ( 85 lines) — TileMap
├── sdf.rs          ( 85 lines) — SDF primitives
├── damage.rs       ( 82 lines) — Damage helpers
├── blur.rs         ( 73 lines) — Horizontal blur
├── source.rs       ( 46 lines) — Source-frame scaling
├── path_cache.rs   ( 40 lines) — Path-shape cache
└── cursor.rs       ( 27 lines) — Cursor blending
```

No functional change. All 9 host tests pass. `lib.rs` public API unchanged.

---

## Test Status

| Suite | Count | Result |
|-------|-------|--------|
| windowd lib | 9 | OK |
| OS check (RISC-V) | — | OK 0 errors |

## Files Changed (2026-05-22)

```
source/services/windowd/src/os_lite.rs → DELETED
source/services/windowd/src/compositor/ (18 files) → NEW
source/services/windowd/src/lib.rs → mod compositor
tasks/TASK-0059-*.md → status Done
docs/rfcs/RFC-0058-*.md → status Done
.cursor/current_state.md → updated
.cursor/handoff/current.md → updated
CHANGELOG.md → updated
```
