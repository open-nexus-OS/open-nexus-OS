# Handoff — TASK-0057 (In Progress, Phase 1+2 complete)

Date: 2026-05-13

## Summary

TASK-0057 builds the complete UI v2b asset pipeline. All 7 implementation phases
are structurally complete. 65 host tests pass across 5 new crates. QEMU markers
are wired into windowd + selftest-client per the Observer pattern.

## What was done

### New crates
- `nexus-theme` (`userspace/ui/theme/`) — .nxtheme.toml parser, schema validation, qualifier resolution. 26 tests.
- `nexus-svg` (`userspace/ui/svg/`) — hand-written XML tokenizer, SVG rich subset parser, tessellator, BGRA8888 scanline rasterizer. Security rejects. 15 tests.
- `nexus-image` (`userspace/ui/image/`) — PNG/JPEG decode via `png`+`jpeg-decoder` crates, bilinear+nearest scaling, decompression bomb detection. 10 tests.
- `nexus-shape` (`userspace/ui/shape/`) — rustybuzz HarfBuzz-compatible shaping, fontdue raster primitives, newtypes (FontId, GlyphIndex, PixelSize). 10 tests.
- `nexus-cursor` (`userspace/ui/cursor/`) — BreezeX cursor loading, SVG rasterization via nexus-svg, hotspot map. 4 tests.

### Renderer integration
- `userspace/ui/renderer/src/draw.rs` — `draw_image()`, `draw_svg()`, `draw_glyph_run()` (stub).

### Resource directory
- `resources/` tree: themes (4 .nxtheme.toml), icons (freedesktop structure), cursors, wallpapers, fonts.

### QEMU markers (Observer pattern)
- `source/services/windowd/src/markers.rs`: `CURSOR_SVG_LOADED_MARKER`, `TEXT_TARGET_VISIBLE_MARKER`, `ICON_TARGET_VISIBLE_MARKER`, `SELFTEST_UI_V2B_ASSETS_OK_MARKER`
- Wired into `source/apps/selftest-client/src/os_lite/phases/end.rs`

### Docs
- `docs/dev/ui/foundations/rendering/image.md` created

## Proofs

```bash
cargo test -p nexus-theme    # 26/26 pass
cargo test -p nexus-svg      # 15/15 pass
cargo test -p nexus-image    # 10/10 pass
cargo test -p nexus-shape    # 10/10 pass
cargo test -p nexus-cursor   # 4/4 pass
cargo test -p ui_renderer    # 2/2 pass (existing)
just dep-gate                # PASS
```

## What remains

### Before claiming Done
- **QEMU asset loading**: windowd needs to actually load cursor SVGs, render text, and render icons during boot. This requires a QEMU test session with `RUN_UNTIL_MARKER=1 just test-os`.
- **Cap'n Proto IPC**: `shape.capnp` schema exists; needs compilation + integration into the OS build.
- **Glyph rasterization**: `draw_glyph_run` is a stub; needs fontdue-backed rasterization via GlyphCache in windowd.
- **Actual font files**: `resources/fonts/inter/` needs Inter-Regular.ttf (currently .gitkeep).
- **Actual cursor SVGs**: `resources/cursors/breezeX/` needs BreezeX SVG cursors.
- **Security reject tests**: `test_reject_decompression_bomb_image` (needs crafted bomb PNG).

## Next task

- Dedicated QEMU session: load assets, fire markers, run `RUN_UNTIL_MARKER=1 just test-os`
- Then: TASK-0059 (scroll, clip, effects, IME/text-input)

## Files changed (this cycle)

- `Cargo.toml` — workspace members: theme, svg, image, shape, cursor
- `resources/` — directory tree + 4 theme files
- `userspace/ui/theme/` — 8 files (new crate)
- `userspace/ui/svg/` — 8 files (new crate)
- `userspace/ui/image/` — 6 files (new crate)
- `userspace/ui/shape/` — 7 files (new crate)
- `userspace/ui/cursor/` — 5 files (new crate)
- `userspace/ui/renderer/` — draw.rs + Cargo.toml + lib.rs
- `source/services/windowd/src/markers.rs` — 4 new markers
- `source/services/windowd/src/lib.rs` — export new markers
- `source/apps/selftest-client/src/os_lite/phases/end.rs` — v2b observer
- `docs/rfcs/RFC-0056-*.md` — checklist updated
- `docs/dev/ui/foundations/rendering/image.md` — new docs
