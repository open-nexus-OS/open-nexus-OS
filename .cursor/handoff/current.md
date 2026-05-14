# Handoff — TASK-0057 (In Progress, Phase 3–5 architecture defined)

Date: 2026-05-14

## Summary

TASK-0057 builds the complete UI v2b asset pipeline. Phase 0-3 complete (83 host tests).
Phase 4 adds manifest.capnp v2.0 with type/dependencies for service auto-discovery.
Phase 5 promotes windowd to standalone IPC service (OHOS WMS model).

## Architecture Insight (corrected)

**Reality**: windowd is used as a LIBRARY by fbdevd and inputd. It is NOT a standalone service.
`os_lite::service_main_loop()` is never called. windowd is NOT in the Makefile service list.

**Target**: windowd as IPC service. inputd sends cursor position via cap-based IPC.
fbdevd queries windowd for composed frame (scanout-only, no own WindowServer).
All per OHOS WMS → DisplayCompositor model.

## What was done (this session)

### Git submodules
- resources/cursors/mocu → sevmeyer/mocu-xcursor (CC0)
- resources/icons/lucide → lucide-icons/lucide (ISC)
- resources/fonts/inter → rsms/inter (SIL OFL)

### Variable font support
- nexus-shape: VariationSettings API, recursive font loading
- InterVariable.ttf works at default coordinates
- 13 tests pass

### no_std cross-compile fix
- nexus-svg: added #[macro_use] extern crate alloc for vec!/format! macros
- windowd cross-compiles for riscv64imac-unknown-none-elf

### Cursor infrastructure (Phase 3)
- smoke.rs: cursor_bitmap in VisibleSystemUiEvidence via render_cursor_surface()
- display_backend.rs: cursor_bitmap in DisplayPresentHandoff
- fbdevd/service.rs: FbdevService stores cursor bitmap
- fbdevd/framebuffer.rs: blend_cursor_row() — BGRA8888 alpha blending
- fbdevd/os_lite.rs: write_live_visible_rows() blends cursor at (cursor_x, cursor_y)
- fbdevd/markers.rs: CURSOR_OVERLAY_ON_MARKER
- markers/ui.toml: fbdevd: cursor overlay on registered

### Architecture docs analysis
- windowd NOT a service (library used by fbdevd/inputd)
- manifest.capnp v1.2 only — no type/dependencies fields
- Makefile hardcodes service list 4× — no auto-discovery
- TRACK-APP-STORE exists but no tasks spawned

## Next: Phase 4 — manifest.capnp v2.0

1. Add type, dependencies, providedServices, resources to manifest.capnp
2. Update nxb-pack for v2.0 schema
3. Update bundlemgrd for v2.0 parsing
4. Create service manifests for windowd, fbdevd, inputd
5. Makefile auto-discovery via cargo metadata

## Next: Phase 5 — windowd as IPC service

1. windowd os_lite.rs: OP_CREATE_SURFACE, OP_COMMIT_SCENE, OP_GET_COMPOSED_FRAME
2. inputd → windowd: CURSOR_POSITION IPC (cap-based)
3. fbdevd queries windowd for composed frame (scanout-only)
4. fbdevd removes own cursor blending
5. Service contract tests per hop

## Proofs

```bash
cargo test -p nexus-theme    # 26/26
cargo test -p nexus-svg      # 15/15
cargo test -p nexus-image    # 10/10
cargo test -p nexus-shape    # 13/13
cargo test -p nexus-cursor   # 4/4
cargo test -p windowd        # 29/29
cargo test -p fbdevd         # 25/25
make build MODE=host         # PASS
make test MODE=host          # PASS (full profile)
just dep-gate                # PASS
```
