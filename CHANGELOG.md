# Changelog

All notable changes to Open Nexus OS will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Fixed - 2026-06-01

#### nexus-init OS build regression (RFC-0061 incomplete refactoring)
- **`source/init/nexus-init/Cargo.toml`**: Added `[[bin]] required-features = ["std-server"]` to prevent RISC-V compilation of host-only binary.
- **`source/init/nexus-init/src/lib.rs`**: Added missing `extern crate alloc;` for `no_std` OS builds.
- **`source/init/nexus-init/src/os_payload.rs`**: Added `pub(crate) use` re-exports (`debug_write_*`, `fatal_err`, `ServiceNameGuard`, `RouteTable`, etc.) for items moved to `bootstrap/` during RFC-0061 refactoring. Made private constants and type aliases `pub(crate)`.
- **`source/init/nexus-init/src/bootstrap/helpers.rs`**: Added `pub(crate)` visibility to functions used by sibling modules. Added missing imports (`LineBuilder`, `log_topics`, `extern` symbols). Made `ServiceNameGuard` struct and fields `pub(crate)`.

#### Compiler warnings
- **gpud/backend.rs**: Prefixed unused closure params with `_`, added `#[allow(dead_code)]` on `ResourceRecord` and `CURSOR_QUEUE_INDEX`.
- **windowd/compositor/backdrop.rs**: Removed unused imports.

#### Proof-manifest
- **markers/ui.toml**: Changed `fbdevd: ready` from `phase=end emit_when={profile=visible-bootstrap}` to `phase=bringup` (fbdevd now starts early per RFC-0059 Phase B).

### Changed - 2026-05-31

#### RFC-0059 Phase 3–6: Production-Grade Display Pipeline

- **gpud**: Display resource upgraded from 64×64 proof to 1280×800 (`DISPLAY_WIDTH`/`DISPLAY_HEIGHT`).
  `virtio-gpu-device` promoted to primary QEMU display (before `ramfb`). New markers:
  `gpud: scanout 1280x800 bgra8888`, `gpud: display ready (w=1280, h=800)`.
- **gpud**: New IPC op `OP_SET_FRAMEBUFFER_VMO` (3) — windowd sends framebuffer VMO for
  zero-copy GPU scanout. `VirtioGpuBackend::attach_external_framebuffer()` attaches external
  VMO as virtio-gpu resource backing, sets as primary scanout.
- **fbdevd**: Boot splash optimized from 800 per-row `vmo_write` calls to single bulk write.
  fbdevd promoted to Priority-0 (3rd service spawned) for <200ms splash visibility.
- **windowd**: Defensive init with wallpaper fallback (solid dark-blue 160×100 when JPEG
  unavailable). New diagnostic markers: `windowd: runtime init start/ok`,
  `windowd: wallpaper loaded (jpeg)`, `windowd: wallpaper fallback solid`.
- **windowd**: `try_handoff_framebuffer_to_gpud()` sends framebuffer VMO to gpud on registration.
  Falls back silently to CPU ramfb path when gpud unreachable.
- **fbdevd**: `register_framebuffer_with_windowd()` exponential backoff (10ms→500ms) with
  diagnostic marker on 3rd retry.

### Changed - 2026-05-22

#### TASK-0059: Compositor module refactoring

- **Refactored**: `source/services/windowd/src/os_lite.rs` (4860 line monolith) split into
  `source/services/windowd/src/compositor/` — 18 focused files with clear ownership boundaries.
  No functional change. All 9 host tests pass. `lib.rs` public API unchanged.
- **Module structure**: `runtime.rs`, `surface.rs`, `backdrop.rs`, `filter.rs`, `shadow.rs`,
  `scene.rs`, `types.rs`, `cache.rs`, `primitives.rs`, `sdf.rs`, `tile_map.rs`, `damage.rs`,
  `blur.rs`, `source.rs`, `path_cache.rs`, `cursor.rs`, `font.rs`, `tests.rs`.
- **Renamed**: `os_lite` → `compositor` throughout (`lib.rs`, module declarations, imports).

### Fixed - 2026-05-21

#### TASK-0059: ShadowCache heap exhaustion on bump allocator

- **Crash fix**: Removed `to_vec()` heap allocation from `compute_shadow_row` hot path.
  Per-row shadow caching with `Vec<u8>` exhausted the 512KB bump allocator (~3500 bytes/row
  × 316 shadow rows = ~1.1MB). Only visible with real display (QEMU `DISPLAY_BACKEND=none`
  skipped the rendering path entirely).
- **Removed**: `ShadowCache` field, import, and all cache get/insert logic from
  `windowd/src/os_lite.rs`. Shadow compositing now executes inline with zero heap allocations
  using pre-allocated `shadow_scratch` + `blur_row_buf`.
- **Added**: `ShadowArena` (64KB pre-allocated buffer pool) in `nexus-effects` for
  production-grade per-box shadow caching (follow-up optimization).
- **Tests**: 8 new tests — `ShadowArena` alloc/reset/overflow/get, alloc-fail prevention
  budget checks, deterministic reset behavior.

### Added - 2026-05-18

#### TASK-0059: UI v3b clip/scroll/effects + IME stub + filter-box proof element

- **Layout engine clip+scroll**: `clip_rect` and `scroll_offset` fields on `LayoutBox`; `Overflow::Hidden` containers propagate scissor rects to children; `compute_scroll_damage()` (bounded, allocation-free) and `LayoutResult::reposition_scroll()` (place-only, no remeasure)
- **TextInputNode**: new `LayoutNode::TextInput` variant with content, cursor_pos, placeholder, and max_length; measures like TextNode
- **Filter-box proof element**: `filter_words()` pure function on 15-word static list; filter-box layout tree (TextInput + `Overflow::Hidden` scrollable word list) integrated into windowd proof panel; 3 cards (hover/click/key) in vertical column, scroll card removed
- **Effects crate (`nexus-effects`)**: box blur (3×3 and 1×3), drop shadow compositing, `EffectBudget` with deterministic degrade, LRU `EffectCache`, `CursorBlink` timer
- **IME stub (`imed`)**: focus routing, `CaretSelection` helpers, caret movement, selection range, 6 unit tests
- **Host tests (`tests/ui_v3b_host/`)**: 23 tests covering scroll damage (4), clip boundaries (2), `filter_words` (6), filter-box layout (3), scroll reposition, effect budget (3), blur (2), cursor blink (2), proof panel filter integration
- **12 OS markers defined** in `windowd/markers.rs` (clipping, scroll, text input, filter, effects, selftest summary)

### Added - 2026-05-19

#### TASK-0059 Phase 6a: Separable blur + shadow properties + two-pass renderer

- **Separable blur (`nexus-effects`)**: `blur_1d()` sliding-window box blur (O(w·h) per pass), `blur_separable()` 2D box blur via horizontal+vertical passes, zero-copy with reused row/transpose buffers
- **Shadow types (`nexus-layout-types`)**: `BoxShadow` (offset, blur_radius, spread, color), `TextShadow` (offset, blur_radius, color), `ShadowLevel` enum (Sm/Md/Lg/Xl/Xxl2) with `to_box_shadow()` Tailwind presets
- **VisualStyle extensions**: `shadow: Option<BoxShadow>`, `text_shadow: Option<TextShadow>`, `opacity` changed to `Option<Fraction>` with `blend_factor()` for alpha compositing
- **Fraction helpers**: `OPAQUE`/`TRANSPARENT` constants, `as_u8()`, `blend_factor()` returning (numerator, 256) for `over` operator
- **Two-pass renderer (`windowd/os_lite.rs`)**: zero-copy `compute_shadow_row()` per-row shadow compositing (alpha mask → horizontal blur → tint → over-composite); `shadow_scratch` + `blur_row_buf` pre-allocated at startup; `blur_row_horizontal()` inline zero-allocation single-row blur
- **Host tests (`tests/ui_v4_host/`)**: 21 tests covering `blur_separable` (2), `blur_1d` (2), `BoxShadow`/`TextShadow` defaults (2), `ShadowLevel` presets (6), `VisualStyle` extensions (5), `Fraction` (4)
- **103 total host tests passing** across layout (9), windowd (31), ui_v3a (13), ui_v3b (20), ui_v4 (21), headless (9)

#### TASK-0059 Phase 6b: MSDF atlas for text and icon rendering

- **MSDF crate (`nexus-msdf`)**: build-time atlas generator rendering 95 printable ASCII glyphs (32-126) as 32×32 signed distance fields via `fontdue` + Inter font; packs into 1024×96 BGRA atlas embedded via `include_bytes!(env!())` for `no_std` compatibility
- **SDF computation**: two-pass 8SSEDT distance transform producing approximated Euclidean signed distance fields (0 = outside, 128 = edge, 255 = inside)
- **Runtime sampler**: `sample_atlas(ch, u, v) -> u8` bilinear-interpolated SDF lookup; `sdf_to_alpha(sd, aa_width) -> u8` smoothstep anti-aliasing; `glyph_metrics(ch) -> Option<&GlyphMetrics>` for advance/bearing/atlas position
- **Zero runtime allocations**: all data in static embedded arrays; `fontdue` only at build time; `no_std` + `alloc` compatible
- **22 host tests**: atlas dimensions/constants (6), glyph metrics lookup (5), SDF sampling correctness (7), sdf_to_alpha math (4)
- **43 total ui_v4_host tests** (21 phase6a + 22 phase6b), dep-gate PASS

#### TASK-0059 Phase 6c: Analytical SDF shapes for anti-aliased rendering

- **SDF crate (`nexus-sdf`)**: `sd_circle`, `sd_rect`, `sd_rounded_rect`, `sd_triangle` analytical signed distance primitives; `smoothstep` cubic Hermite interpolation; `fill_alpha`/`border_alpha` rendering combinators; `rounded_rect_fill_alpha`/`rounded_rect_border_alpha` convenience functions; `no_std` + `libm`, zero allocations, deterministic
- **Renderer integration (`windowd/os_lite.rs`)**: `fill_sdf_circle_row`/`stroke_sdf_circle_row` replace hard-edged `fill_circle_row`/`stroke_circle_row` for anti-aliased circles; `fill_sdf_rounded_rect_row`/`stroke_sdf_rounded_rect_row` used for `ShapeKind::Rect` with `corner_radius > 0`; hard-edged rects keep fast `fill_row_rect` span-fill path
- **23 SDF host tests**: circle (4), rect (3), rounded rect (4), triangle (3), smoothstep (3), fill/border alpha (4), rounded rect convenience (2)
- **66 total ui_v4_host tests** (21 phase6a + 22 phase6b + 23 phase6c), 148 total host tests, dep-gate PASS

#### TASK-0059 Phase 6d: 9-slice shadow compositing

- **9-slice shadow (`nexus-effects`)**: `NineSliceShadow` decomposition (corner_size, blur_radius, spread, color); `composite_nine_slice_shadow()` renders 4 corners with 2D separable blur, 4 edges by stretching blurred corner columns/rows, center fill with solid shadow alpha — ~90% fewer blur ops than full-surface; `EffectCache` integration with compound key `(elem_w, elem_h, params)`
- **Bug fix**: `blur_1d` vertical pass used wrong stride (`w*4` instead of `h*4`) for transposed buffer; fixed
- **8 host tests**: basic output, zero-size noop, budget exhaustion, corner blur verification, center fill solidity, cache hit/miss, different params → different cache keys, area ratio vs full-surface blur
- **74 total ui_v4_host tests** (21+22+23+8), 156 total host tests, dep-gate PASS

#### TASK-0059 Phase 6e: Dual-kawase blur

- **Dual-kawase blur (`nexus-effects`)**: `dual_kawase_blur()` — downscale pyramid (2× box-filter per level), iterative `stride_blur_3x3` with configurable sample step (1, 2, 4, …), bilinear upscale reconstruction; O(log(radius)) samples/pixel vs O(radius²) for box blur; `stride_blur_3x3` underflow fix for `isize` offset arithmetic
- **7 host tests**: identity (r=0, iter=0), solid color preservation, edge blur spread, small image noop, iteration comparison, large radius 48×48
- **81 total ui_v4_host tests** (21+22+23+8+7), 163 total host tests, dep-gate PASS

#### TASK-0059 Phase 6f: Render cache + damage integration

- **Specialized caches (`nexus-effects`)**: `ShadowCache` (256-entry LRU, keyed by node_id_hash + params, per-node invalidation), `TextCache` (512-entry LRU, keyed by glyph_id + scale_bucket, per-scale invalidation); existing `EffectCache` retained for 9-slice backward compat; `RenderCache` aggregator with `begin_frame()`, `invalidate_dirty()` (shadows cleared on dirty, text survives), `note_scroll()` (no invalidation), `clear()` (full clear on theme change)
- **15 host tests**: ShadowCache (insert/get, miss, update, LRU eviction, node invalidation, clear), TextCache (insert/get, miss, LRU eviction, scale invalidation), RenderCache (clear, dirty invalidate, scroll preserve, no-dirty no-op, begin_frame)
- **96 total ui_v4_host tests** (21+22+23+8+7+15), 170+ total host tests, dep-gate PASS
- **RFC-0058 Phase 6 complete** — NeX UI Rendering Pipeline fully implemented

### Fixed - 2026-05-20

- **Budgeted first-frame glass quality**: `write_current_frame` now calls `select_glass_quality(self.mode.height)` instead of forced `GlassQuality::High`. On 800-row screens this degrades to `Opaque` (no blur), preventing the high-quality backdrop blur from blocking boot scanout. Previously caused black-screen QEMU boot.
- **Test string contract fix**: `windowd_first_frame_uses_budgeted_glass_quality` assertion updated from 3-arg to 4-arg `write_rows` call to include the `paint_only: false` parameter.

### Added - 2026-05-15

#### RFC-0057: UI v3a layout engine contract seed (pretext philosophy)

- Created design seed for the deterministic layout engine (`docs/rfcs/RFC-0057-ui-v3a-layout-engine-pretext-contract.md`):
  - Rust type system: `Stack` (flex row/column), `Grid` (fraction columns), `Spacer`, `FlexItem`, `EdgeInsets`
  - `MeasureText` callback trait decoupling layout from `nexus-shape` (pure Rust: rustybuzz + fontdue, no C libs)
  - Naming aligned with DSL v0.1a (`Stack` not VStack/HStack; `padding`/`margin`/`gap` mirror modifiers)
  - Paragraph/run cache + line-layout cache split following chenglou/pretext prepare/layout philosophy
  - Fixed-point arithmetic (no `f32`/`f64` in layout math)
  - windowd proof panel replacement contract (hardcoded positions → layout-tree-driven)
  - Invalidation matrix for TASK-0059 scroll-as-place-only handoff
- TASK-0058 updated: concrete types, pretext reference, shape cache integration, windowd integration plan
- TASK-0059 updated: `depends-on: [TASK-0058]`, pretext reuse for scroll damage math, place-only contract
- RFC-0057 v2: Visual primitives (`Rgba8`, `Border`, `EdgeBorder`, `CornerRadius`, `VisualStyle`), Text styling (`TextAlign`, `LineHeight`, `FontWeight`, `WhiteSpace`, `TextStyle`), Container features (`Overflow`, `Position`, `ZIndex`, `flex_wrap`, `row_gap`), Theme token integration contract
- Phases restructured: 0=Container layout, 1=Visual+Text primitives, 2=Text wrapping+caches, 3=Host tests, 4=windowd
- TASK-0058: `flex_wrap`, `Position`, `ZIndex`, `row_gap`, `WhiteSpace` added to type system
- RFC-0057 status: Draft → In Progress; TASK-0058: In Progress (implementation starting)
- .cursor files synced: current_state, next_task_prep, context_bundles, pre_flight, stop_conditions

### Added - 2026-05-17

#### TASK-0058 **DONE** — production-grade layout engine
- 31 host tests, windowd integrated, no duplicate structure
- ProofPaintRole system + proof_box_rect guard clause for allocation-free rendering
- RFC-0057: Done

### Added - 2026-05-16

#### TASK-0058 impl done (31 tests)
- nexus-layout-types + nexus-layout (Flex+Grid engine)
- nexus-shape wrap.rs (UAX#14) + cache.rs
- tests/ui_v3a_host JSON goldens (4 tests)
- windowd: layout_panel.rs integrated into os_lite.rs (single source of truth, no duplicate structure)

### Changed - 2026-05-11

### Added - 2026-05-17

#### TASK-0058 **DONE** — production-grade layout engine
- 31 host tests, windowd integrated, no duplicate structure
- ProofPaintRole system + proof_box_rect guard clause for allocation-free rendering
- RFC-0057: Done

### Added - 2026-05-16

#### TASK-0058 impl done (31 tests)
- nexus-layout-types + nexus-layout (Flex+Grid engine)
- nexus-shape wrap.rs (UAX#14) + cache.rs
- tests/ui_v3a_host JSON goldens (4 tests)

### Changed - 2026-05-11

#### TASK-0056C / RFC-0055 present-input perf latency coalescing (`TASK-0056C`, `RFC-0055`)

- Closed the embedded reactor/runtime floor for present-input perf with deterministic latency coalescing:
  - `windowd` now implements deterministic pointer-motion burst coalescing (bounded batch + latest-wins) while preserving click, focus, wheel, and keyboard edges as individually observable events
  - `windowd` implements explicit no-damage frame skip (frame-level hash match, max 3 consecutive, forced present on 4th)
  - `windowd` implements explicit no-visible-state-change skip (semantic state, bounded counter, requires at least 1 frame shown)
  - All skip decisions check both damage and visible-state before skipping; if either is true, present proceeds
  - Added idle-cheap / wakeup-collapse telemetry and stable counter infrastructure
- Authority boundaries preserved: `inputd` normalizes input, `windowd` decides compose/skip/present, `fbdevd` handles cadence/scanout
- Proof package `tests/ui_v2c_host` with 22 host tests (coalescing, skip rules, reject-edge, boundedness assertions)
- `RFC-0055` promoted to Complete; implementation checklist fully checked
- QEMU marker ladder (56C perf markers) remains deferred to follow-up; `just diag-os` RISC-V build passed clean

### Changed - 2026-04-29

#### TASK-0055B / RFC-0048 visible QEMU scanout bootstrap (`TASK-0055B`, `RFC-0048`)

- Closed the narrow visible-bootstrap slice with a deterministic QEMU `ramfb` first-frame path:
  - `scripts/run-qemu-rv64.sh` now has an opt-in `NEXUS_DISPLAY_BOOTSTRAP=1` graphics path (`-display gtk`, `-device ramfb`) while preserving headless default runs
  - `nexus-init` grants `selftest-client` a policy-gated `device.mmio.fwcfg` capability for QEMU `fw_cfg` access
  - `selftest-client` writes the fixed `1280x800` ARGB8888 framebuffer VMO and configures `etc/ramfb` through `fw_cfg` DMA
  - `windowd` owns the fixed visible bootstrap mode, pattern, present evidence, and fail-closed marker gating
  - proof-manifest profile `visible-bootstrap` is explicitly a harness/marker profile, not a SystemUI/launcher start profile
- Added proof coverage for visible bootstrap mode/capability/pre-scanout rejects and QEMU marker validation:
  - `cargo test -p windowd -p ui_windowd_host -- --nocapture`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap`
- Visible SystemUI/launcher profile selection, input routing, cursor, dirty-rect display service behavior, virtio-gpu, perf budgets, and kernel/core production-grade display closure remain follow-up scope.

### Changed - 2026-04-27

#### TASK-0055 / RFC-0047 headless windowd present closure (`TASK-0055`, `RFC-0047`)

- Closed the headless `windowd` surface/layer/present slice after critical remediation (`RFC-0047` Done, `TASK-0055` Done):
  - `source/services/windowd` now owns bounded surface IDs, VMO-shaped buffer validation, layer commits, damage-aware composition, and minimal present acknowledgements
  - `source/services/windowd/src/lib.rs` is now a facade over focused modules instead of a monolith
  - `tests/ui_windowd_host` proves exact two-surface composition, no-damage present skip, deterministic layer ordering, present acknowledgements, generated Cap'n Proto roundtrips, vsync/input-stub behavior, atomic commit preservation, and expanded reject paths
  - `userspace/apps/launcher` is now the canonical `launcher` package; the old `source/apps/launcher` placeholder was removed
  - `selftest-client`, proof-manifest markers, `scripts/qemu-test.sh`, and `tools/postflight-ui.sh` now gate honest UI present markers
- Added proof coverage:
  - `cargo test -p windowd -p ui_windowd_host -p launcher -p selftest-client -- --nocapture`
  - `cargo test -p ui_windowd_host reject -- --nocapture`
  - `cargo test -p ui_windowd_host capnp -- --nocapture`
  - `cargo test -p launcher -- --nocapture`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
  - `scripts/fmt-clippy-deny.sh`
  - `make build` → `make test`
  - `make build` → `make run`
- Visible scanout, real input routing, GPU/display-driver work, rich display presets, and kernel/MM/IPC/zero-copy production closure remain follow-up scope.
- VMO scope is explicitly limited to UI-shaped `windowd` handle/rights/byte-length validation; no new kernel VMO capability-transfer or zero-copy production claim is made.

#### TASK-0054 / RFC-0046 host renderer closure (`TASK-0054`, `RFC-0046`)

- Closed the narrow host-first UI renderer proof floor and RFC contract:
  - `userspace/ui/renderer` provides a safe Rust BGRA8888 `Frame`, checked dimensions/stride/damage newtypes,
    deterministic clear/rect/rounded-rect/blit/text primitives, and bounded full-frame damage overflow behavior
  - `userspace/ui/fonts` provides the repo-owned deterministic fixture font; no host font discovery or locale fallback
  - `tests/ui_host_snap` proves expected pixels, full rounded-rect/text masks, damage behavior, snapshot/golden
    comparison, PNG metadata independence, golden update gating, artifact path confinement, anti-fake-marker source
    scanning, and required reject classes
- Added host proof coverage:
  - `cargo test -p ui_renderer -- --nocapture`
  - `cargo test -p ui_host_snap -- --nocapture`
  - `cargo test -p ui_host_snap reject -- --nocapture`
  - `just diag-host`
  - `just test-all`
  - `just ci-network`
  - `scripts/fmt-clippy-deny.sh`
  - `make clean`, `make build`, `make test`, `make run`
- Synchronized `TASK-0054` to `Done`, `RFC-0046` to `Done`, RFC index, status board, implementation order, and UI testing docs.
- OS/QEMU present markers, compositor/windowd wiring, GPU/device paths, and Gate A kernel/core production-grade claims remain out of scope.

### Changed - 2026-04-26

#### TASK-0047 / RFC-0045 host-first closure (`TASK-0047`, `RFC-0045`)

- Closed the Policy as Code v1 host-first contract floor:
  - active policy root is now `policies/nexus.policy.toml`
  - `recipes/policy/` is legacy documentation only, not a live TOML authority
  - `userspace/policy` provides deterministic `PolicyVersion`, bounded evaluator traces, and stable reject classes
  - Config v1 carries policy candidate roots as `policy.root`
  - `policies/manifest.json` records the deterministic tree hash and validates fail-closed when missing or stale
  - `policyd` stages configd-fed `PolicyTree` candidates through `configd::ConfigConsumer` and rejects stale/unauthorized lifecycle changes
  - external `policyd` host frame operations for `Version`, `Eval`, `ModeGet`, and `ModeSet` are backed by `PolicyAuthority` and bounded audit events
  - the `policyd` service-facing check frame evaluates through the unified authority
  - `nx policy` lives under `tools/nx` with deterministic JSON/exit contracts; `nx policy mode` is explicit host preflight only
- Added host proof coverage:
  - `cargo test -p policy -- --nocapture`
  - `cargo test -p nexus-config -- --nocapture`
  - `cargo test -p configd -- --nocapture`
  - `cargo test -p policyd -- --nocapture`
  - `cargo test -p nx -- --nocapture`
- Synchronized Policy as Code architecture docs and added a local `tools/nx/README.md` entrypoint for the canonical CLI.
- OS/QEMU policy markers remain gated and intentionally unclaimed.

### Changed - 2026-04-24

#### TASK-0046 / RFC-0044 closure sync (`TASK-0046`, `RFC-0044`)

- Closed the Config v1 host-first contract floor:
  - JSON-only authoring for layered config sources under `/system/config` and `/state/config`
  - canonical Cap'n Proto effective snapshots remain the runtime/persistence authority
  - `configd` subscriber/update notification seam is covered by deterministic host tests
  - `nx config push` now writes deterministic state overlay `state/config/90-nx-config.json`
- Added closure-proof coverage:
  - lexical-order layer-directory merge proof in `nexus-config`
  - non-JSON authoring reject proof in `nexus-config`
  - `nx config reload --json` and `nx config where --json` contract tests
  - `nx config effective --json` parity proof against `configd` version + derived JSON
- Synchronized status/index/queue surfaces:
  - `tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md` → `In Review`
  - `docs/rfcs/RFC-0044-config-v1-configd-schema-layering-2pc-host-first-os-gated.md` → `Done`
  - `docs/rfcs/README.md`, `tasks/IMPLEMENTATION-ORDER.md`, `tasks/STATUS-BOARD.md`
  - `.cursor/current_state.md`, `.cursor/handoff/current.md`, `.cursor/next_task_prep.md`, `.cursor/pre_flight.md`, `.cursor/stop_conditions.md`, `.cursor/context_bundles.md`
- Normalized touched Rust source headers to the documented standard (`OWNERS` / `STATUS` / `API_STABILITY` / `TEST_COVERAGE` / `ADR`) and refreshed docs to describe the current proof state.

### Changed - 2026-04-23

#### TASK-0032 / RFC-0041 status synchronization (`TASK-0032`, `RFC-0041`)

- Updated execution/contract status to the requested review state:
  - `tasks/TASK-0032-packagefs-v2-ro-image-index-fastpath.md` → `status: In Review`
  - `docs/rfcs/RFC-0041-packagefs-v2-ro-image-index-fastpath-host-first-os-gated.md` → `Status: Done`
- Synced RFC index wording in `docs/rfcs/README.md`:
  - `RFC-0041` now tracked as `Done`
  - execution SSOT `TASK-0032` now tracked as `In Review`
- Synced task tracking views:
  - `tasks/IMPLEMENTATION-ORDER.md` now has an `In Review` section with `TASK-0032`
  - `tasks/STATUS-BOARD.md` queue head and contract-status lines now point to `TASK-0032` / `RFC-0041`
  - `tasks/STATUS-BOARD.md` cumulative done table now includes `TASK-0029` and `TASK-0031`
- Updated packaging documentation `docs/packaging/nxb.md` with explicit `pkgimg-build` / `pkgimg-verify` usage notes for PackageFS v2 image generation and verification.

### Changed - 2026-04-23

#### TASK-0032 prep sync + queue/workfile alignment (`TASK-0029`, `TASK-0031`, `TASK-0032`, `RFC-0041`)

- Added `TASK-0029` and `TASK-0031` to the cumulative Done table in `tasks/IMPLEMENTATION-ORDER.md`.
- Created RFC seed contract for the active SSOT task:
  - `docs/rfcs/RFC-0041-packagefs-v2-ro-image-index-fastpath-host-first-os-gated.md`
- Linked the new seed from `tasks/TASK-0032-packagefs-v2-ro-image-index-fastpath.md` and updated `docs/rfcs/README.md` index entries.
- Synced active task prep workfiles for `TASK-0032` posture:
  - `.cursor/context_bundles.md`
  - `.cursor/pre_flight.md`
  - `.cursor/stop_conditions.md`

### Changed - 2026-04-20

#### TASK-0023B Phase 6 functional closure + RFC-0038 → Done (`TASK-0023B`, `RFC-0038`)

- `TASK-0023B` advanced from `Draft` to `In Review` after Phase 6 (replay capability) reached functional closure across all six cuts.
- `RFC-0038` advanced from `Draft` to `Done`. One environmental closure step remains and is documented inline in the RFC header: external CI-runner replay artifact for P6-05; recipe lives in `docs/testing/replay-and-bisect.md` §7-§11.
- Phase 6 deliverables (cuts P6-01 → P6-06) shipped:
  - `tools/replay-evidence.sh` — bounded `--max-seconds` replay with hard env-override gate (`PROFILE` / `SELFTEST_PROFILE` / `RUN_PHASE` / `REQUIRE_*` / `KERNEL_CMDLINE` rejected), persistent worktree (`target/replay-worktree`) + Cargo cache reuse, automatic `NEXUS_SKIP_BUILD=1` warm-replay (cold ~67s, warm ~14s on dev box), structured logs, deterministic `nexus-evidence` / `nexus-proof-manifest` binary resolution.
  - `tools/diff-traces.sh` + `docs/testing/trace-diff-format.md` + `docs/testing/trace-diff-fixtures.json` — phase-aware classifier with `exact_match` / `extra_marker` / `missing_marker` / `reorder` / `phase_mismatch` classes.
  - `tools/bisect-evidence.sh` — bounded binary-search bisect with mandatory `--max-commits` + `--max-seconds`; synthetic mode extended to `good | drift | bad` so allowlist-absorbed drift is reported separately from regressions.
  - `scripts/regression-bisect.sh` — CI-friendly wrapper.
  - `docs/testing/replay-and-bisect.md` — operator workflow, append-only allowlist policy, evidence-map (§9), synthetic bad-bundle reproducer (§10), and the explicit remaining environmental step (§11).
- Phase-6 proof floor verified locally with reproducible artifacts:
  - empty-diff replay vs good bundle on native (`.cursor/replay-dev-a.json`) and containerized CI-like host (`.cursor/replay-ci-like.json`),
  - synthetic bad-bundle (tampered + re-sealed) classified diff with non-zero exit (`.cursor/replay-synthetic-bad.{log,json}` — `status: "diff", classes: ["missing_marker"]`),
  - 3-commit good→drift→regress bisect smoke (`.cursor/bisect-good-drift-regress.json` — `first_bad_commit: c2cccccc`, `drift_commits: [c1bbbbbb]`),
  - all hard gates verified (`--max-seconds`/`--max-commits` mandatory exits; `PROFILE` env override rejected with explicit error).
- Status synchronized across:
  - `docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md`
  - `docs/rfcs/README.md`
  - `tasks/TASK-0023B-selftest-client-production-grade-deterministic-test-architecture-refactor.md`
  - `tasks/STATUS-BOARD.md`
  - `tasks/IMPLEMENTATION-ORDER.md`
  - `docs/adr/0027-selftest-client-two-axis-architecture.md` (Current state section refreshed; ADR remains `Accepted` because Phase 4-6 work consumes the two-axis structure rather than altering it)
  - `docs/testing/index.md` (RFC-0038 added to Related RFCs; topic guides extended with §9-§11 anchors)
  - `source/apps/selftest-client/README.md` (Status section rewritten with full P1-P6 closure table + remaining environmental closure step)
  - `.cursor/handoff/current.md`, `.cursor/current_state.md`, `.cursor/next_task_prep.md`
- Sequencing: queue head moves to `TASK-0024` (DSoftBus QUIC recovery / UDP-sec) once the external CI-runner replay artifact for P6-05 is captured and the documented status flip is applied.

### Changed - 2026-04-15

#### TASK-0023 gate-prep sync (`TASK-0023`)

- Archived `.cursor/handoff/current.md` snapshot to `.cursor/handoff/archive/TASK-0022-dsoftbus-core-no-std-transport-refactor.md`.
- Synchronized `TASK-0023` to explicit blocked-state truth:
  - follow-up routing now explicit (`TASK-0024`, `TASK-0044`),
  - RED feasibility point resolved as documented gate outcome,
  - security proof test names aligned to existing host reject suites.
- Updated active workfiles and queue docs for production-grade anti-drift clarity (`.cursor/current_state.md`, `.cursor/handoff/current.md`, `.cursor/next_task_prep.md`, `.cursor/pre_flight.md`, `.cursor/stop_conditions.md`, `tasks/IMPLEMENTATION-ORDER.md`, `tasks/STATUS-BOARD.md`).
- Synced architecture/distributed docs that still referenced `TASK-0022` review state:
  - `docs/architecture/README.md`
  - `docs/adr/0005-dsoftbus-architecture.md`
  - `docs/distributed/dsoftbus-lite.md`

#### TASK-0022 closure sync (`TASK-0022`, `RFC-0036`)

- `TASK-0022` is now `Done` after final production-quality verification and closure sync.
- `RFC-0036` is `Complete` and remains aligned as the closed contract seed for this slice.
- `TASK-0023` gated-contract closure is now done with blocked/no-go unlock outcome; sequential queue head is `TASK-0024` unless resequenced.
- `dsoftbus-core` crate boundary and review evidence synchronized into process docs:
  - `tasks/IMPLEMENTATION-ORDER.md`
  - `tasks/STATUS-BOARD.md`
  - `.cursor/current_state.md`
  - `.cursor/handoff/current.md`
  - `.cursor/next_task_prep.md`
- Fresh quality/security/performance verification pass run:
  - `cargo +nightly-2025-01-15 check -p dsoftbus-core --target riscv64imac-unknown-none-elf`
  - `cargo test -p dsoftbus --test core_contract_rejects -- --nocapture`
  - `cargo test -p dsoftbus -- reject --nocapture`
  - `just test-dsoftbus-quic`
  - `just deny-check`
  - `just dep-gate && just diag-os`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
  - `just test-e2e && just test-os-dhcp`

### Changed - 2026-04-14

#### DSoftBus QUIC host-first closure sync (`TASK-0021`, `RFC-0035`)

- `TASK-0021` advanced from `In Review` to `Done`.
- Queue head advanced to `TASK-0022`.
- Closure state synchronized across task/board/workfiles:
  - `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`
  - `tasks/STATUS-BOARD.md`
  - `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
  - `.cursor/current_state.md`
  - `.cursor/handoff/current.md`
  - `.cursor/next_task_prep.md`
  - `README.md`
- Cargo-deny duplicate handling is now explicit and strict:
  - `multiple-versions = "deny"` remains enforced,
  - narrow compatibility skips were added only for `getrandom` (`0.2/0.3`) and `windows-sys` (`0.52/0.61`).
- Fresh green gate evidence includes:
  - `just test-os-dhcp`
  - `just test-dsoftbus-host`
  - `just test-all`
  - `just deny-check`

### Changed - 2026-04-10

#### DSoftBus mux v2 production closure (`TASK-0020`, `RFC-0033`, `RFC-0034`)

- `TASK-0020` is closed as `Done` with host, single-VM, and 2-VM marker proofs plus deterministic perf/soak and release-evidence artifacts.
- `RFC-0033` status is now `Complete` (mux v2 contract closure).
- `RFC-0034` status is now `Complete` for legacy `TASK-0001..0020` production-closure scope.
- Sequential queue head moved to `TASK-0021` after `TASK-0020` closeout.

### Changed - 2026-03-27

#### DSoftBus mux v2 kickoff (`TASK-0020`, `RFC-0033`)

- Verified `TASK-0019` closeout remains documented as `Done` across task status, board views, and changelog evidence.
- Moved `TASK-0020` to `In Progress` as the active sequential queue head.
- Moved `RFC-0033` to `In Progress` with `TASK-0020` as execution SSOT.
- Synced working-state artifacts for active execution context:
  - `.cursor/current_state.md`
  - `.cursor/handoff/current.md`
  - `.cursor/next_task_prep.md`
  - `.cursor/pre_flight.md`
  - `.cursor/stop_conditions.md`
  - `.cursor/context_bundles.md`

### Changed - 2026-03-27

#### ABI syscall guardrails v2 closeout (`TASK-0019`, `RFC-0032`)

- `TASK-0019` status advanced from `In Review` to `Done` after closing host/OS/QEMU proof gates.
- Workspace/task status sources were synchronized for drift-free closure:
  - `.cursor/current_state.md`
  - `.cursor/handoff/current.md`
  - `.cursor/next_task_prep.md`
  - `.cursor/pre_flight.md`
  - `.cursor/stop_conditions.md`
  - `tasks/IMPLEMENTATION-ORDER.md`
  - `tasks/STATUS-BOARD.md`
  - `tasks/TASK-0019-security-v2-userland-abi-syscall-filters.md`
- Root documentation now reflects closure and queue progression:
  - `README.md` (TASK-0019 done, next queue head TASK-0020)
- Additional green gate verification for this closeout:
  - `make build MODE=host`
  - `make test MODE=host`
  - `make run MODE=host RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s`

### Changed - 2026-03-26

#### Crashdump v1 final hardening closure sync (`TASK-0018`, `RFC-0031`)

- `TASK-0018` final hardening slice is now reflected across implementation + proof docs:
  - identity/report validation is fail-closed and deterministic,
  - explicit negative E2E markers are part of the canonical QEMU ladder:
    - `SELFTEST: minidump forged metadata rejected`
    - `SELFTEST: minidump no-artifact metadata rejected`
    - `SELFTEST: minidump mismatched build_id rejected`
- `execd` crash publish path now validates reported metadata against decoded bounded minidump bytes before emitting `execd: minidump written`.
- `statefsd` crash-write subject canonicalization is documented and unit-tested as a pure helper (narrow, path-bound mapping only; no broad SID-0 bypass).
- Task planning/status artifacts were synchronized for queue visibility and anti-drift:
  - `tasks/IMPLEMENTATION-ORDER.md`
  - `tasks/STATUS-BOARD.md`
  - `.cursor` SSOT/handoff/pre-flight/stop-conditions files
- Verification set for this sync includes:
  - `cargo test -p crash -- --nocapture`
  - `cargo test -p execd -- --nocapture`
  - `cargo test -p minidump-host -- --nocapture`
  - `cargo test -p statefsd -- --nocapture`
  - `just dep-gate`
  - `just diag-os`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`

### Changed - 2026-03-24

#### Networking modularization + address governance closure sync (`TASK-0016B`, `RFC-0029`, `ADR-0026`)

- `netstackd` modular refactor closure is now synchronized in docs and task/rfc state:
  - `main.rs` is entry/wiring only, with runtime split under `source/services/netstackd/src/os/**`.
  - handler and IPC helper seams are now the canonical extension points for follow-on networking tasks.
- Networking address/profile governance is now explicit and centralized:
  - `docs/architecture/network-address-matrix.md` is the SSOT for QEMU + os2vm address profiles.
  - `docs/adr/0026-network-address-profiles-and-validation.md` records policy-level decisions.
- DNS proof validation remains deterministic but is now protocol-semantic (port/QR/TXID) rather than source-IP-pinned, avoiding backend-specific false negatives.
- Task board and implementation-order docs were refreshed to match real task/RFC status progression (`TASK-0016` Done, `TASK-0016B` Complete, `RFC-0028` Completed, `RFC-0029` Completed).
- Verification set for this sync includes:
  - `just dep-gate`
  - `just diag-os`
  - `just test-os-dhcp-strict`
  - `RUN_OS2VM=1 RUN_TIMEOUT=180s OS2VM_PROFILE=ci RUN_PHASE=end tools/os2vm.sh`

### Changed - 2026-02-11

#### Perf/Power v1 closure (TASK-0013; RFC-0023 implemented)

- Kernel QoS syscall decode now deterministically rejects malformed/overflowed wire args with `-EINVAL` (no silent clamp).
- QoS authority model enforced and audited: self-set allows equal/lower only, escalation requires privileged `policyd/execd` path.
- New `timed` service path operational in OS bring-up with deterministic coalescing windows and bounded registration limits.
- Proof ladder extended and validated with deterministic markers, including negative over-limit and reject-path checks.
- Address-space/page-table lifecycle hardening landed during closure debugging to remove `KPGF`/allocation leak regressions in QEMU runs.

### Changed - 2026-02-10

#### Kernel SMP v1 closure sync (TASK-0012 Done; RFC-0021 Complete)

- Hardened SMP v1 proof semantics from marker-presence to causal anti-fake evidence:
  - `request accepted -> send_ipi success -> S_SOFT trap observed -> ack`
- Added deterministic SMP counterfactual proof marker:
  - `KSELFTEST: ipi counterfactual ok`
- Added/validated required SMP negative proof markers:
  - `KSELFTEST: test_reject_invalid_ipi_target_cpu ok`
  - `KSELFTEST: test_reject_offline_cpu_resched ok`
  - `KSELFTEST: test_reject_steal_above_bound ok`
  - `KSELFTEST: test_reject_steal_higher_qos ok`
- Canonical SMP harness gate now explicitly uses `REQUIRE_SMP=1` for SMP marker ladder runs.
- Documentation synchronized across task/rfc/testing/architecture/handoff to preserve drift-free follow-up prerequisites for TASK-0013/0042/0247/0283.

#### Build/QEMU reliability sync (default marker-driven run + blk lock serialization)

- `make run` now defaults to marker-driven mode (`RUN_UNTIL_MARKER=1`) so default runs complete green when the selftest ladder reaches `SELFTEST: end`.
- Added serialized lock handling for shared QEMU block image access in `scripts/run-qemu-rv64.sh` to avoid concurrent `blk.img` write-lock failures.

### Added - 2026-01-14

#### Observability v1 (TASK-0006: Complete)

**New Services**:
- `logd`: Bounded RAM journal for structured logs
  - Wire protocol v1: APPEND/QUERY/STATS (versioned byte frames for OS, Cap'n Proto for host)
  - Ring buffer semantics: drop-oldest on overflow, deterministic counters
  - Authenticated origin: `sender_service_id` from kernel IPC metadata
  - RFC: `docs/rfcs/RFC-0011-logd-journal-crash-v1.md` (Complete)

**Logging Integration**:
- `nexus-log` extended with `logd` sink (`sink-logd` feature)
- Core services integrated: `samgrd`, `bundlemgrd`, `policyd`, `dsoftbusd`
- Existing UART readiness markers preserved for deterministic testing
- Fallback: UART-only if `logd` unavailable

**Crash Reporting**:
- `execd` crash reporting for non-zero exits
  - UART marker: `execd: crash report pid=<pid> code=<code> name=<name>`
  - Structured crash event appended to `logd` (queryable for post-mortem)
  - Stable crash event keys: `event=crash.v1`, `pid`, `code`, `name`, `recent_count`
  - Reserved keys for future: `build_id`, `dump_path`

**Testing**:
- Host tests: `cargo test -p logd`, `cargo test -p nexus-log`
- QEMU markers (all green as of 2026-01-14):
  - `logd: ready`
  - `SELFTEST: log query ok`
  - `SELFTEST: core services log ok`
  - `execd: crash report pid=... code=42 name=demo.exit42`
  - `SELFTEST: crash report ok`

**Documentation**:
- New: `docs/observability/logging.md` (usage guide)
- New: `docs/rfcs/RFC-0011-logd-journal-crash-v1.md` (contract seed)
- Updated: `docs/architecture/` (10+ files), `docs/testing/index.md`, ADR-0017

**Demo Payloads**:
- `demo.exit42` added to `userspace/apps/demo-exit0` for crash report testing

**Breaking Changes**: None (additive only)

**Known Limitations (v1 scope)**:
- Journal is RAM-only (no persistence)
- No streaming/subscriptions (bounded queries only)
- No remote export (deferred to TASK-0040)
- No metrics/tracing integration (deferred to TASK-0014)

### Added - 2026-01-25

#### Policy authority + audit baseline v1 (TASK-0008: Done; RFC-0015: Complete)

- `policyd` established as the **single policy authority** with deny-by-default semantics.
- Audit trail for allow/deny decisions (via `logd`), binding authorization to kernel `sender_service_id`.
- Policy-gated sensitive operations (baseline): signing/exec/install paths enforced without duplicating authority logic.
- Contract: `docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md`

### Added - 2026-01-27

#### Device identity keys v1 (TASK-0008B: Done; RFC-0016: Done)

- OS/QEMU device identity key generation path proved without `getrandom`:
  - virtio-rng MMIO → `rngd` (entropy authority) → `keystored` (device keygen + pubkey-only export).
- Bounded entropy requests and negative proofs (oversized/denied/private-export reject); no secrets logged.
- Contract: `docs/rfcs/RFC-0016-device-identity-keys-v1.md`

### Added - 2026-02-02

#### Device MMIO access model v1 (TASK-0010: Done; RFC-0017: Done)

- Kernel/userspace contract for capability-gated device MMIO mapping (`DeviceMmio` + mapping syscall).
- Enforced security floor: USER|RW mappings only, never executable; bounded per-device windows; init/policyd control distribution.
- Contract: `docs/rfcs/RFC-0017-device-mmio-access-model-v1.md`

### Added - 2026-02-06

#### Persistence v1 (TASK-0009: Done; RFC-0018: Complete; RFC-0019: Complete)

- StateFS journal format v1 + `/state` authority service (`statefsd`) with deterministic host + QEMU proofs.
- IPC request/reply correlation v1 (nonces + bounded reply buffering) to keep shared-inbox flows deterministic under QEMU.
- Modern virtio-mmio default for virtio-blk in the canonical QEMU harness (legacy remains opt-in).
- Contracts:
  - `docs/rfcs/RFC-0018-statefs-journal-format-v1.md`
  - `docs/rfcs/RFC-0019-ipc-request-reply-correlation-v1.md`

### Changed - 2026-02-09

#### Kernel simplification (TASK-0011: Complete; RFC-0001: Complete)

- Kernel tree reorganized into stable responsibility-aligned directories (mechanical moves + wiring only).
- Kernel module headers normalized; invariants and test scope made explicit to lower debug/navigation cost.
- Contract: `docs/rfcs/RFC-0001-kernel-simplification.md`

---

## Previous Releases

See Git history for releases prior to 2026-01-14.