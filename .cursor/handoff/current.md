# Handoff - TASK-0057 Minimal DisplayServer v0

Date: 2026-05-15

## Current State

TASK-0057 is now a service-owned visible output slice:

```text
hidrawd -> inputd -> windowd -> fbdevd -> ramfb
```

`windowd` is the Minimal DisplayServer v0 authority. It owns the root scene,
hit-test/focus state, JPEG wallpaper, Mocu cursor, Inter proof text, proof
targets, and row composition into the framebuffer VMO registered by `fbdevd`.

`fbdevd` is scanout-only. It allocates/configures the framebuffer, transfers a
framebuffer VMO capability to `windowd`, waits for `STATUS_OK`, then reports
scanout/overlay evidence. It must not reintroduce a second cursor truth.

`inputd` owns normalized input state and sends bounded `OP_UPDATE_VISIBLE_STATE`
frames to `windowd`. Hover/click/key/scroll target states are transient:
hover only while routed over the target, click only while primary pointer is
held, keyboard only while a non-modifier key is held, and scroll up/down pulses
are distinct and expire on a bounded input tick.

`selftest-client` is observer-only. It emits summary markers only after observing
service-owned state from the display/input chain.

## Assets

- Wallpaper: `resources/wallpapers/base/default.jpeg`, decoded/scaled at build
  time by `systemui` and embedded as BGRA for deterministic OS use.
- Cursor: `resources/cursors/mocu/src/svg/default.svg`, normalized by
  `windowd/build.rs` into the bounded SVG subset. The normalized asset preserves
  Mocu shadow/stroke/fill colors and renders as a stride-compatible 32px cursor for the
  1280x800 visible mode.
- Text: `resources/fonts/inter/docs/font-files/InterVariable.ttf`, rasterized at
  `windowd` build time into the proof overlay. This replaces the former
  hardcoded atlas fallback for the visible OS proof.
- Icons: proof icon remains windowd-owned scene content.

## Important Files

- `docs/rfcs/RFC-0056-ui-v2b-asset-theme-cursor-text-pipeline.md`
- `docs/architecture/display-output-service-chain.md`
- `docs/testing/display-output-hardening-matrix.md`
- `docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md`
- `source/services/windowd/build.rs`
- `source/services/windowd/src/os_lite.rs`
- `source/services/windowd/src/assets.rs`
- `source/services/inputd/src/os_lite.rs`
- `source/services/fbdevd/src/os_lite.rs`
- `userspace/input-live-protocol/src/lib.rs`
- `userspace/ui/svg/tests/cursor_golden.rs`

## Verified Proofs

Focused proofs run after the latest cursor/text/target changes:

```bash
cargo +nightly-2025-01-15 test -p nexus-svg --test cursor_golden -- --nocapture
RUSTFLAGS='--cfg nexus_env="os"' cargo +nightly-2025-01-15 build -p windowd --target riscv64imac-unknown-none-elf --release --no-default-features --features os-lite
RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap
```

`visible-bootstrap` reaches:

- `display: mode 1280x800 argb8888`
- `windowd: cursor svg loaded`
- `windowd: wallpaper visible`
- `windowd: text target visible`
- `windowd: icon target visible`
- `fbdevd: cursor overlay on`
- `SELFTEST: ui v2b assets ok`

## Remaining Caution

The current OS proof uses build-time rasterization for Inter text and a
build-normalized Mocu SVG cursor because the minimal OS renderer still lacks
production-quality support for all upstream SVG features. Full runtime
HarfBuzz-in-OS, richer icon rendering, animated cursors, GPU acceleration, IME,
and multi-window WM behavior remain follow-up scope.
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
