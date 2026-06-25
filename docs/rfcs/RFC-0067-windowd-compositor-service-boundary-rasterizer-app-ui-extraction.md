# RFC-0067: windowd — clean compositor-service boundary (rasterizer → NexusGfx, app/shell UI → userspace)

- Status: **Done** (2026-06-25) — windowd is a layer-compositor service; the inline CPU shadow, the demo content, and the CPU base-scene bake are gone. See **Outcome** below for what shipped and the small deferred residue.
- Owners: @ui @runtime
- Created: 2026-06-24
- Links:
  - Motivation: `source/services/windowd` has grown to ~22k LOC and conflates three concerns — a display-server **service**, an inline **2D rasterizer**, and **app/shell UI content**. The service is no longer cleanly debuggable.
  - Builds on / aligns with: `tasks/TASK-0169` + `tasks/TASK-0170` (renderer abstraction: Scene-IR + Backend trait + cpu2d), `tasks/TASK-0170B` (NexusGfx windowd handoff), `tasks/TRACK-NEXUSGFX-SDK.md`, RFC-0059 (NexusGfx SDK structure), RFC-0063/0064/0065 (the UI stack this slims down).
  - Reuse anchors (already windowd deps): `userspace/nexus-gfx` (`command/buffer.rs`, `backend/cpu_mock.rs`), `userspace/ui/sdf`, `userspace/ui/effects`, `userspace/ui/shells/desktop`, `userspace/ui/widgets/virtual_list`, `userspace/apps/search` (`search-app`).
  - ADR: this RFC folds the service-boundary decision (no separate ADR per the owner's request).

## Problem — why windowd is hard to debug

A display server should be a **thin compositor service**: own surfaces, damage, present scheduling, the GPU-driver handoff, and input routing — and nothing else. windowd today also contains:

1. **A full inline 2D rasterizer.** `compositor/{sdf,shadow,blur,backdrop,primitives,surface,scene,source,path_cache}.rs` + `fixed_sdf.rs` + `frame.rs` — SDF shapes, soft shadows, blur, glass backdrop, row compositing. **The smoking gun:** windowd *already depends on* `nexus-sdf`, `nexus-effects`, `nexus-gfx`, `nexus-svg`, `nexus-theme` — so the same primitives exist **three times** (windowd-inline vs `ui/sdf`+`ui/effects` vs `nexus-gfx/backend/cpu_mock`, which already has `fill_sdf_gradient`/`fill_sdf_rounded`/`blit`).
2. **App + shell UI content.** Chat (`compositor/chat.rs`), the filter word-list demo (`compositor/filter.rs`, `proof_panel_spec.rs`, `layout_panel.rs`), the desktop topbar/sidepanel (`compositor/desktop_layer.rs`), and the reusable glass-window component (`compositor/shell_window.rs`, `window_frame.rs`, `window_scene.rs`). `search` content was *already* extracted to `userspace/apps/search` — windowd keeps only chrome. That extraction is the template for the rest.
3. **Two-to-three parallel scene models.** Retained `scene_graph.rs` (1703 LOC) + `systemui_shell.rs`, vs the live `nexus-gfx` `CommandBuffer` path (`runtime/scene.rs::build_scene_cb_into`), vs the half-wired `proof_panel`/`layout_panel` demo (still referenced from `runtime/mod.rs` and `interaction.rs`).

The result: changing one pixel can mean touching a rasterizer, an app, and a scene model that all live in the "window server". That is the opposite of debuggable.

## Design principles (the boundary)

The throughline across mature display stacks: **the compositor is not the renderer, and not the app.** What we adopt:

- **Compositor scope:** windowd composites client-owned surfaces + drives present; it does **not** rasterize app content.
- **Rasterization home:** the NexusGfx SDK — one backend (cpu2d now, GPU later) behind `CommandBuffer`.
- **App UI:** app content lives in `userspace/apps/<app>` (model + render).
- **Reusable widgets:** `userspace/ui/widgets` + `userspace/ui/shells/*`.
- **The seam:** VMO surface + `nexus-gfx` `CommandBuffer` + the gpud present spine.

We already have every destination crate; they are just used inconsistently.

## Target boundary — "what belongs where"

- **windowd (service, stays):** IPC server/protocol (`server.rs`), present scheduling/pacing + gpud client (`runtime/{gpud,present,framebuffer}.rs`), damage/tile/cache (`tile_map`, `damage`, `cache`), atlas/VMO surface lifecycle (`atlas`, `app_surface`, `buffer`, `resource_pool`), input routing + hit-test SSOT (`interaction`, `runtime/input.rs`), window management (`runtime/{chat_window,search,shell,scroll,cursor,anim}.rs`), and **per-frame Scene build → backend submit** (`runtime/scene.rs`). Markers/telemetry/smoke. Nothing else.
- **The rasterization stack (layered — *revised after the P5 audit*):**
  - **`nexus-sdf`** = the one **SDF math SSOT** (float analytic + the fixed-point/no-FPU sibling, now relocated here from windowd). Definitions only.
  - **`nexus-effects`** = the one **effects math SSOT** (blur / drop-shadow / 9-slice).
  - **`nexus-gfx`** = the portable **command contract** (`Command{FillSdfGradient, DropShadow, BlurBackdrop, …}`) + **one** CPU command-executor (derived from `nexus-sdf`/`nexus-effects`).
  - **`gpud` virgl shaders** = the **primary** renderer — SDF/gradient/shadow as GPU fragment shaders, executing the same commands.
  - windowd **emits** the command buffer; it does **not** CPU-rasterize the UI. This is finishing RFC-0063 ("scene graph is the only rendering authority"), which deleted the CPU compositor on paper but never in code.
- **`userspace/apps/<app>`:** app content. `search` already there → `chat` joins it.
- **`userspace/ui/`:** the reusable glass-window component (→ `ui/widgets`) and desktop shell chrome (→ `ui/shells/desktop`, already a dep as `nexus-shell-desktop`).

## Authoritative module map (keep / move / delete)

| windowd module(s) | Fate | Destination / phase |
|---|---|---|
| `server.rs`, `lib.rs`, `main.rs`, `cli.rs`, `error.rs`, `ids.rs`, `geometry.rs` | **KEEP** | service core |
| `runtime/{gpud,present,framebuffer,input,scene,scroll,cursor,anim,marker_emit}.rs`, `compositor/mod.rs` | **KEEP** | service runtime |
| `atlas.rs`, `buffer.rs`, `resource_pool.rs`, `app_surface.rs`, `display_backend.rs` | **KEEP** | surface/VMO/present plane |
| `compositor/{tile_map,damage,cache}.rs`, `interaction.rs`, `live_runtime.rs`, `visible_state.rs`, `markers*.rs`, `telemetry.rs`, `smoke.rs`, `registry_client.rs` | **KEEP** | damage/input/proof |
| `legacy.rs` (`render_frame`, only `pub use`d, never called) | **DELETE** | P1 |
| `compositor/{sdf,shadow,blur,backdrop,primitives,surface,scene,source,path_cache}.rs`, `fixed_sdf.rs`, `frame.rs` | **MOVE** | NexusGfx cpu2d backend / P5 |
| `compositor/chat.rs` (CPU chat content), `compositor/filter.rs` (word-list) | **MOVE** | `userspace/apps/chat` (+ retire dup) / P2 |
| `compositor/shell_window.rs`, `window_frame.rs`, `window_scene.rs` | **MOVE** | `userspace/ui/widgets` / P3 |
| `compositor/desktop_layer.rs` (topbar/sidepanel), `app_menu.rs`, `desktop_scene.rs` | **MOVE** | `userspace/ui/shells/desktop` / P3 |
| `scene_graph.rs`, `systemui_shell.rs`, `proof_panel_spec.rs`, `layout_panel.rs` | **INVESTIGATE → DELETE/MOVE** | scene-model collapse / P4 |
| `assets.rs`, `render_assets.rs`, `bitmap_font.rs`, `compositor/font.rs` | **KEEP (revisit)** | assets — may join `ui/` later |

## Phased gates

**Gate contract (every phase):** host workspace build green · windowd host tests green (`tests/headless.rs`, `tests/damage_pipeline.rs`, `src/compositor/tests.rs`) · riscv os-lite `-p windowd` build green · **owner boots `GPU_MODE=virgl just start` and confirms the UI is visually identical** before committing. Strangler-fig: add the new home → switch the consumer → delete the old copy → prove identical → commit. Never two live copies past a gate.

- **P0 — RFC + safety net (this doc; no code moves).** Land RFC-0067 + the module map. Confirm the host oracle (headless/damage/compositor tests) is green as the regression baseline. *Gate: docs land, tests green.*
- **P1 — Delete provable dead/legacy.** `legacy.rs` + its `pub use`; prune dead re-exports. Smallest, safest first.
- **P2 — Widget foundation + scroll SSOT, then chat content (foundation-first).** Diagnosis: there is **no `List` widget**; `VirtualList` is a standalone one-off (not an extension of a layout-primitive List); scroll physics has a real SSOT (`animation::ScrollMomentum`, an Android `OverScroller` analog) but windowd runs **two parallel scroll mechanisms on top of it** — the chat overscan/`chat_render_base`/recenter-re-render bookkeeping and the hardcoded-step `runtime/scroll.rs::handle_scroll_input` — which is the actual cause of the chat-freeze (#72) and overscan-re-render (#74) bugs. Android-clean target: layout primitives (LayoutManager) → `List` (finite) → `VirtualList` = `List` + lazy windowing (RecyclerView) → `ScrollMomentum` as the one scroll SSOT (OverScroller), with windowd's GPU row-offset reduced to a pure *presentation* of the SSOT offset. Sub-phases, each host-tested + boot-gated:
  - **P2.1** — new `List` widget on `nexus-layout` primitives (finite, non-lazy; the base).
  - **P2.2** — refactor `VirtualList` to extend `List` + lazy windowing/recycling (no behavior change to its embedders).
  - **P2.3** — make `ScrollMomentum` the sole scroll SSOT consumed by `List`/`VirtualList`; route windowd's chat/search/filter scroll through it; retire the overscan + hardcoded-step parallel paths (fixes #72/#74).
  - **P2.4** — chat content → new `userspace/apps/chat` (full consolidation: `ChatMessage` + `ChatMessageProvider` + message `POOL` + `ChatItemView` moved out of the generic `nexus-virtual-list` widget and `nexus-shell-desktop`); `virtual_list` becomes purely generic; windowd/shell consume `chat-app`. windowd keeps only window chrome + surface upload (the OwnedSurface present swap stays parked under ADR-0037, same as search).
- **P3 — Window component + shell chrome → `userspace/ui`.** `shell_window`/`window_frame`/`window_scene` → `ui/widgets`; `desktop_layer`/`app_menu`/`desktop_scene` → `ui/shells/desktop`.
- **P4 — Collapse the scene-model parallel.** Lock the canonical per-frame path (`nexus-gfx` `CommandBuffer` Scene, per TASK-0170) and retire the dead parallel (`scene_graph.rs` and/or the `proof_panel`/`layout_panel` demo), keeping the live desktop-shell scene.
- **P5 — Finish RFC-0063: GPU-command-primary + collapse the CPU rasterizers (rasterization SSOT).** *Revised after the P5 audit found SDF/effects computed in **six** places (gpud virgl shaders, windowd inline CPU, gpud `cpu_vector`, nexus-gfx `cpu_mock`, `nexus-sdf`, `nexus-effects`) — and that nexus-gfx's `cpu_mock` is a **coarse no-AA reference**, while windowd's inline path is the production AA one. A blind swap regresses to jagged edges; the direction reverses.* The target:
  - **Math SSOT** in `nexus-sdf` (float + fixed-point) + `nexus-effects`; the GPU shaders and the CPU executor both derive from them.
  - **GPU/virgl shaders are the primary renderer**; windowd emits `Command{FillSdfGradient, DropShadow, BlurBackdrop, …}` for the whole UI instead of CPU-rasterizing into the VMO.
  - **One** CPU command-executor (collapse gpud `cpu_vector` + nexus-gfx `cpu_mock` + windowd's inline rasterizer into a single nexus-sdf/effects-derived fallback for mmio/host/golden).
  - **Delete** windowd `compositor/{sdf,shadow,backdrop,surface,source,scene,primitives}` + `fixed_sdf` (the CPU compositor RFC-0063 condemned).
  - **P5.1–5.4 DONE:** the SDF math SSOT is `nexus-sdf::fixed`; the CPU executors derive from it — windowd rows (P5.1), nexus-gfx `cpu_mock` (P5.2), gpud `cpu_vector` (P5.4). The only hot-path-risky phase — boot-iterated, small reversible steps with markers (I can't repro the live path host-side).

### P5-Final — finish RFC-0063 (GPU-primary), measured not guessed

Inventory finding: the GPU command path (`build_scene_cb_into`) is **already primary** — it emits `FillSdfGradient`/`BlurBackdrop`/`DropShadow`/`CompositeLayer`/`BlitSurface` unconditionally; gpud runs them on GPU (virgl) or CPU (`cpu_vector`). The CPU's remaining job is **content rasterization** (text glyphs + wallpaper) into surfaces the GPU composites — blocked from GPU by the absence of a **glyph command**. So windowd's `compositor/{sdf,shadow,backdrop,blur,surface,scene,source}.rs` are a *mix* of live content-helpers, mmio-only fallback, and possibly-dead; **blind deletion is unsafe**.

Strict gates (each: host + riscv green; **owner boots `GPU_MODE=virgl just start`**; commit before next):

- **G0 — Measure (markers only, zero visual risk).** Add a one-shot marker at each CPU rasterizer entry (`sdf`/`shadow`/`backdrop`/`blur`/`surface` row fns). Owner boots virgl → the markers that **don't** fire are not on the virgl path = candidates to delete/consolidate.
- **G1…Gn — Delete/consolidate the proven-unused-on-virgl CPU rasterizers**, one per gate, boot-verifying virgl is pixel-identical (and mmio if it matters). Anything still live on virgl stays.
- **G-text (separate track, the real blocker):** a GPU glyph/text command so text rasterizes on the GPU; only then can the CPU content path (and the last of the CPU compositor) be retired. Out of P5-Final scope — flagged.
- **Outcome:** windowd's compositor = content source + present + GPU commands; the redundant CPU glass rasterizers gone; CPU reduced to content (text/wallpaper) pending G-text.
- **P6 — Final shape + docs.** Refresh `//! CONTEXT` headers + `compositor/mod.rs` doc; confirm the slimmed tree; mark this RFC Done. *Gate: full green + boot.*

## Outcome — DONE (2026-06-25)

windowd is now a **layer-compositor service**: the CPU only rasterizes content into surfaces; the GPU composites every visual element as a layer. The boundary the RFC set out to draw is in place.

**Shipped (host + riscv green, boot-verified by the owner):**

- **P0–P2 — done.** RFC + safety net; `legacy.rs` and the `compositor` crate deleted; chat/search content extracted to `userspace/apps/*`; the scroll SSOT landed (`animation::ScrollMomentum`; chat/search unified on `VirtualList`).
- **P4 — done.** The CPU base-scene parallel is collapsed: **Plane 1 holds only the wallpaper.** The proof panel + its shadow are gone; `compute_shadow_row` and the per-row CPU content bake are deleted. No more "change one pixel, touch a rasterizer + an app + a scene model."
- **P5 / P5-Final — done.** SDF math relocated to `nexus-sdf` (float + fixed-point sibling); the **layer-compositor SSOT** landed in `nexus-gfx` (`command/layer.rs` `Layer` + `composite_layer_full`, 9 host tests) and **every** glass surface (chat, search, topbar, side panel, dropdown, and the now-deleted proof panel) routes through it — the 5-way copy-pasted glass recipe collapsed to one. **Real frosted blur** runs on the virgl scanout (`gl_scanout.rs::blur_rt_backdrop`, the build-up RT FS_BLUR gaussian per glass layer) instead of the old opacity tint.
- **Demo-content teardown (the "closure" pass) — done.** The proof/target-test subsystem is fully deleted from windowd: render, input/scroll/hit-test wiring, `runtime/scroll.rs`, the `proof_layouts`/`filtered_words` runtime state, `compositor/surface.rs`'s rasterizer (gutted to the live layer-cache fns), `compositor/filter.rs` (gutted to the marker-counter), and `src/layout_panel.rs` (deleted). Dimension constants were inlined; lib exports + obsolete tests removed.

**Deferred residue (small, tracked — not blocking the boundary):**

- **P3 (shell UI → `userspace/ui`):** `compositor/shell_window.rs` + `compositor/desktop_layer.rs` still live in windowd (they're coupled to `AtlasSurface`/`WindowdError`). Moving them is the last "windowd contains app/shell UI render code" item — a clean follow-up, not a regression.
- **`src/proof_panel_spec.rs` kept** as **build-time data only**: `build.rs` `include!`s it to pre-render Inter-font text glyphs (asset data + colour tokens). It is no longer in the lib module tree and contains no live proof logic; gutting the build-time text-atlas pipeline is a separate pass.
- **Blur refinements** (separable H+V pass, 140% saturation, blurring layered content vs the wallpaper texture) live in gpud, not windowd.
- Pre-existing bugs (#72/#74 scroll, #69 full-frame black) and the systemui→windowd scene handoff (#67/#83) are independent tracks.

**Net:** windowd = IPC server + present/pacing + damage/atlas + input routing + per-frame Scene→`nexus-gfx` layers. No inline CPU shadow, no demo panel, no CPU base-scene bake. The RFC's boundary is reached; the residue above is follow-up polish, not the conflation this RFC removed.

## Verification (per phase)

- Host: `RUSTFLAGS='… nexus_env="host"' cargo test -p windowd` (+ any new app/ui crate), then `cargo build --workspace --exclude neuron --exclude neuron-boot`.
- riscv: `RUSTFLAGS='… nexus_env="os"' cargo +nightly-2025-01-15 build -p windowd --no-default-features --features os-lite --target riscv64imac-unknown-none-elf --release`.
- QEMU (the real gate, owner-run): `GPU_MODE=virgl just start` → UI visually identical; chat/search open + scroll, hover, launcher, cursor unchanged. Owner commits per phase.

## Non-goals

- No behavior or visual change in any phase — this is a structural refactor with identical output.
- No GPU backend work (cpu2d stays default); the NexusGfx seam keeps a future GPU backend pluggable but does not build it here.
- No new app features; `chat`/`search` extraction preserves current content only.
