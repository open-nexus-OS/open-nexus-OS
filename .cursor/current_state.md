# Current State — Open Nexus OS

Last updated: 2026-05-13 (TASK-0057 Phase 1+2 complete, QEMU markers wired)

## Active task

TASK-0057: UI v2b asset pipeline + theme system + SVG/PNG/JPG + text shaping + cursor pipeline.
Status: In Progress. RFC-0056: In Progress (checklist all checked, QEMU run pending).

## Completed phases

- Phase 0: Resource directory + nexus-theme crate (26 tests)
- Phase 1a: nexus-svg crate — parser, tessellator, rasterizer (15 tests)
- Phase 1b: nexus-image crate — PNG/JPEG decode + scale (10 tests)
- Phase 1c: nexus-shape crate — rustybuzz shaping (10 tests)
- Phase 2a: nexus-cursor crate — BreezeX cursor pipeline (4 tests)
- Phase 2b: Renderer draw.rs integration (draw_image, draw_svg, draw_glyph_run)
- Phase 2c: QEMU markers in windowd + selftest-client (Observer pattern)

## Open before Done

- QEMU asset loading + marker fire: needs QEMU session
- Font files: resources/fonts/inter/ needs Inter-Regular.ttf
- Cursor SVGs: resources/cursors/breezeX/ needs BreezeX SVG assets
- Cap'n Proto: shape.capnp schema exists, compilation deferred
- draw_glyph_run: stub, needs fontdue rasterization in windowd
- test_reject_decompression_bomb_image: needs crafted test asset

## Known risks

- DON'T add prints/logs/markers in kernel
- HarfBuzz via rustybuzz — pure Rust, no C deps
- JPG codec via jpeg-decoder crate; OS path needs no_std alternative