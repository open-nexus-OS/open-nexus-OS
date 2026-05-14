# Current State — Open Nexus OS

Last updated: 2026-05-14 (TASK-0057 Phase 4b complete: auto-discovery + manifest v2.0)

## Active task

TASK-0057: UI v2b asset pipeline + manifest v2.0 + windowd IPC service.
Status: In Progress (Phase 0–4 complete, Phase 5 pending).
RFC-0056: In Progress (Phase 3–5 scope defined).

## Completed phases

### Phase 0-2: Asset Crates + Integration
- Phase 0: Resource directory + nexus-theme crate (26 tests)
- Phase 1a: nexus-svg crate — no_std cross-compile fixed (15 tests)
- Phase 1b: nexus-image crate (10 tests)
- Phase 1c: nexus-shape crate — variable font support, recursive font loading (13 tests)
- Phase 2a: nexus-cursor crate (4 tests)
- Phase 2b: Renderer draw.rs integration
- Phase 2c: QEMU markers wired (observer-only)
- Git submodules: mocu-xcursor (CC0), lucide-icons (ISC), inter (SIL OFL)

### Phase 3: Cursor Live-Blending (fbdevd)
- 3a: cursor_bitmap in DisplayPresentHandoff + VisibleSystemUiEvidence
- 3b: FbdevService stores bitmap, blend_cursor_row() BGRA8888 alpha blending
- 3c: write_live_visible_rows() blends cursor at (cursor_x, cursor_y) from VisibleState
- Marker: `fbdevd: cursor overlay on`
- 83/83 host tests pass (windowd 29, fbdevd 25, nexus-shape 13, nexus-svg 15, nexus-cursor 4, nexus-image 10, nexus-theme 26 removed = wait, let me recount)

### Phase 4a: manifest.capnp v2.0
- Schema extended: bundleType (app|service|library|driver|framework), dependencies, providedServices, resources (6 ResourceKind values)
- nxb-pack updated: TOML → capnp binary with v2.0 fields
- Service manifests created for windowd (library), fbdevd, inputd, bundlemgrd
- schema_version = 2

### Phase 4b: Service Auto-Discovery
- scripts/discover-services.sh: reads cargo metadata, filters [package.metadata.nexus-service]
- Modes: --list, --build-args, --env-vars, --dep-gate-list
- 24 Cargo.tomls updated with [package.metadata.nexus-service] (stack_pages, kind)
- Makefile: 4 hardcoded service lists replaced with auto-discovery
- OS_SKIP filter for services not yet cross-compilable (identityd, debugsvc, virtioblkd)
- kind=library filter (windowd not built as standalone binary)
- Host tests: 4/4 nx::init_lite_input_service_startup pass
- make clean && make build && make test MODE=host: ALL GREEN

## Architecture decisions (OHOS-aligned)

```
windowd = library (used by fbdevd, inputd) — NOT a standalone service
fbdevd  = service (scanout owner, depends on windowd)
inputd  = service (input routing, depends on windowd)

Display chain: hidrawd → inputd → fbdevd → ramfb
  windowd is a library call within fbdevd (bootstrap_display_handoff)
  and within inputd (LiveRouteRuntime WindowServer)
```

## Pending: Phase 5 (windowd as IPC service)

### 5a: windowd IPC service main loop
- os_lite::service_main_loop() handles: OP_CREATE_SURFACE, OP_COMMIT_SCENE, OP_GET_COMPOSED_FRAME
- Cursor position tracking from inputd IPC
- Composes full scene including cursor at live position

### 5b: inputd → windowd cursor position IPC
- inputd sends CURSOR_POSITION(x, y) via cap-based IPC each frame
- inputd removes own WindowServer (delegates to windowd)

### 5c: fbdevd scanout-only
- fbdevd queries windowd (not inputd) for composed frame
- Removes own cursor blending (now done by windowd)
- Becomes pure "dumb scanout owner"

### Phase 5 tests needed
- IPC contract tests: windowd create_surface, commit_scene, get_composed_frame
- inputd → windowd cursor position IPC test
- fbdevd → windowd composed frame query test
- Service contract: blend_cursor_row → correct pixels at (x,y)

### Phase 5 docs needed
- docs/architecture/display-output-service-chain.md: update with windowd service
- ADR update: windowd library → service migration
- manifest.capnp comments: document v2.0 usage

## Proofs

```bash
cargo test -p nexus-theme    # 26/26
cargo test -p nexus-svg      # 15/15
cargo test -p nexus-image    # 10/10
cargo test -p nexus-shape    # 13/13
cargo test -p nexus-cursor   # 4/4
cargo test -p windowd        # 29/29
cargo test -p fbdevd         # 25/25
cargo test -p nxb-pack       # 1/1
cargo test -p nx --test init_lite_input_service_startup  # 4/4
make clean && make build MODE=host && make test MODE=host  # ALL GREEN
just dep-gate                # PASS
```

## Files changed (this cycle)

### Phase 3 (cursor blending)
- source/services/windowd/src/smoke.rs (cursor_bitmap in evidence)
- source/services/windowd/src/display_backend.rs (cursor fields in handoff)
- source/services/windowd/src/os_lite.rs (cursor commit + service loop)
- source/services/fbdevd/src/service.rs (cursor bitmap storage)
- source/services/fbdevd/src/backend/framebuffer.rs (blend_cursor_row)
- source/services/fbdevd/src/os_lite.rs (live blending)
- source/services/fbdevd/src/markers.rs (CURSOR_OVERLAY_ON_MARKER)
- source/apps/selftest-client/proof-manifest/markers/ui.toml

### Phase 3 (no_std fixes)
- userspace/ui/svg/src/lib.rs (#[macro_use] extern crate alloc)
- userspace/ui/svg/src/parse.rs (clean imports)
- userspace/ui/svg/src/tessellate.rs (vec! macro import)
- userspace/ui/svg/src/raster.rs (vec! macro import)

### Phase 4a (manifest v2.0)
- tools/nexus-idl/schemas/manifest.capnp (BundleType, Dependency, Resource, ResourceKind)
- tools/nxb-pack/src/main.rs (v2.0 TOML → capnp compilation)
- resources/manifests/ (new: windowd, fbdevd, inputd, bundlemgrd manifests)

### Phase 4b (auto-discovery)
- scripts/discover-services.sh (new)
- 24 source/services/*/Cargo.toml ([package.metadata.nexus-service])
- source/apps/selftest-client/Cargo.toml (metadata)
- Makefile (auto-discovery replaces 4 hardcoded lists)
- tools/nx/tests/init_lite_input_service_startup.rs (updated for auto-discovery)

### Docs
- docs/rfcs/RFC-0056-ui-v2b-asset-theme-cursor-text-pipeline.md (Phase 3-5 scope)
- tasks/TASK-0057-ui-v2b-text-shaping-svg-pipeline.md (extended)
- .cursor/current_state.md (this file)
- .cursor/handoff/current.md (updated)

## Known risks
- DON'T add prints/logs/markers in kernel
- windowd used as LIBRARY by fbdevd/inputd — Phase 5 changes this
- GTK-based visible-bootstrap tests require X11/Wayland display
- identityd/debugsvc/virtioblkd excluded from OS build (OS_SKIP)
- manifest.capnp v2.0 schema needs bundlemgrd parser update (deferred)
