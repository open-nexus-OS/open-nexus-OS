---
title: TASK-0063 UI v5b: virtualized list + scene graph wiring + dual-panel blur + virgl GPU rendering + 120 Hz pacing + theme tokens
status: Done
owner: @ui
created: 2025-12-23
updated: 2026-06-22 (closed: scene graph wiring + virtual list + theme tokens + virgl GPU pipeline + soft-real-time pacing all landed and boot-verified over virgl)
depends-on: [TASK-0059, TASK-0062]
follow-up-tasks: [TASK-0275, TASK-0064]
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - UI v5a runtime baseline: tasks/TASK-0062-ui-v5a-reactive-runtime-animation-transitions.md
  - UI v3b scroll/clip baseline: tasks/TASK-0059-ui-v3b-clip-scroll-effects-ime-textinput.md
  - UI v4a tiling baseline: tasks/TASK-0060-ui-v4a-tiled-compositor-clipstack-atlases-perf.md
  - UI layout pipeline contract: docs/dev/ui/foundations/layout/layout-pipeline.md
  - Pretext layout engine: docs/rfcs/RFC-0057-ui-v3a-layout-engine-pretext-contract.md
  - Clip/scroll/effects contract: docs/rfcs/RFC-0058-ui-v3b-clip-scroll-effects-ime-contract.md
  - Hardening plan (GPU pipeline + scene graph): docs/architecture/hardening-plan.md
  - Platform-class UI perf matrix: docs/dev/perf/PLATFORM-CLASS-UI-PERFORMANCE-OPTIMIZATIONS-QEMU-MATRIX.md
  - Lazy loading contract: docs/dev/ui/collections/lazy-loading.md
  - Virtual list contract: docs/dev/ui/collections/widgets/virtual-list.md
  - Lazy loading follow-up: tasks/TASK-0275-ui-v5c-lazy-data-loading-virtual-list-paging-contract.md
  - WM + scene transitions: tasks/TASK-0064-ui-v6a-window-management-scene-transitions.md
  - Config broker (theme overrides): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Testing contract: scripts/qemu-test.sh
  - Design contract (RFC): docs/rfcs/RFC-0063-ui-v5b-scene-graph-gpu-pipeline-virtual-list-theme-contract.md
---

## Closure (2026-06-22) — ist-zustand

DONE and boot-verified over `GPU_MODE=virgl just start`:

- **Scene graph wired**: `flush_pending_damage` drives the retained scene; the old CPU row-compositor
  monoliths were deleted and the path refactored into `compositor/` submodules (ShellWindow +
  `window_frame`). OS build green for `riscv64`.
- **Virtual list**: `nexus-virtual-list` (`VirtualList<P: ItemProvider>`) backs the live chat scroll
  with stable visible range, recycling, and the shared Android `ScrollMomentum` fling/tick physics.
- **Theme tokens**: `nexus-theme` registry with 2PC-ready switching; glass tint/edge tokens consumed
  by the compositor.
- **Dual-panel glass + GPU blur**: glass panels composite over a cached blurred backdrop
  (`composite_scrollable_glass`); the blur runs on the GPU via the virgl 3D pipeline
  (`gl_scanout`/`virgl_composite`, `submit_layer_pass`), CPU box-blur is the runtime-selected fallback.
- **Pacing**: soft-real-time spine landed (RFC-0033: waitset + timeline fence + kernel timer IRQ,
  syscalls 38–43); the windowd timer-capped present path is 120 Hz-capable. virgl present quirk
  contained by the multi-entry command ring + batched present (`RING_SLOTS=16`).

The original RED flag (120 Hz honestly requires virgl) is satisfied: the virgl GPU profile is
functional; the CPU profile honestly targets 60 Hz with `gpud: cpu fallback`. Follow-ups that are NOT
part of this task's DoD: chat scroll freeze polish (#72), systemui-as-boot-service (#83), full-frame
`gl_present_damage` black-screen debug (#69) — tracked separately. WM + scene transitions continue in
TASK-0064.

## Context

After v5a establishes the reactive runtime and timeline, v5b adds the virtualized list primitive and theme tokens.
However, the current compositor uses a CPU row-compositing path (`write_rows` → `draw_proof_surface_row` →
per-row `blur_backdrop_segment`) that cannot sustain 120 Hz under blur load. The scene graph
(`scene_graph.rs`, 834 lines, fully built) and SystemUI shell (`systemui_shell.rs`, 299 lines, fully built)
already exist but are not wired into the frame path — `flush_pending_damage` still uses the old CPU path.

The hardening plan (`docs/architecture/hardening-plan.md`, ~4.5h estimated) defines the exact sequence to
remove CPU compositing and unify into a GPU-only pipeline driven by the scene graph. This task must
execute that plan as its first phase so the virtual list, blur, and pacing work all target the correct
architecture from day one.

### Architecture (4-plane VMO + scene graph)

The 16MB VMO is laid out as 4 planes (1280×3200):
- **Plane 0** (rows 0–799): Wallpaper source — static, written once at boot
- **Plane 1** (rows 800–1599): Retained scene — currently CPU-rendered; target: GPU-rendered from scene graph CB
- **Plane 2** (rows 1600–2399): Frame ring slot A — GPU blit target (active scanout)
- **Plane 3** (rows 2400–3199): Frame ring slot B — alternate scanout

The scene graph (`SceneNode` + `RenderPrimitive` + `InvalidationClass`) is the canonical retained tree.
All UI frontends (native widgets, design kit, DSL interpreter, AOT) target this single tree.
`InvalidationClass` (MeasureAndPlace / PlaceOnly / PaintOnly / Clean) propagates upward;
`compute_dirty_set()` uses subtree content hashing for O(1) damage skipping.

gpud is a virtio-gpu driver. Without virgl, `BlurBackdrop` executes as CPU box-blur (`blur_backdrop_vmo`,
5KB stack scratch, per-row loop). With virgl, the same `BlurBackdrop` CB command dispatches to a real
GPU fragment shader — no CPU row loop.

### Why this matters for 120 Hz

- **CPU blur path**: per-row box-blur on the emulated RISC-V CPU is the bottleneck. Dual-panel blur doubles it.
- **Scene graph path**: `compute_dirty_set()` produces only the changed nodes. Scroll = PlaceOnly on the list
  container — no text reshaping, no per-row CPU loop. The GPU translates the blit rect.
- **Virgl path**: `BlurBackdrop` becomes a GPU separable gaussian shader. The RISC-V side sends the same CB;
  the execution path changes. This is the only realistic path to 120 Hz with blur active.

### Pretext layout alignment

The pretext prepare/layout split (RFC-0057) means text measurement is cached and decoupled from placement.
Virtual list recycling reuses cached paragraph/run measurements — no redundant text shaping on scroll or
page load. The scene graph's `Text` primitive receives pre-shaped content; layout is upstream.

## Goal

Deliver:

1. **Scene graph wired**: execute hardening plan Phases A1–A4 + Phase C. `flush_pending_damage` calls
   `shell.graph.compute_dirty_set()` → `generate_commands()` → GPU CB. No CPU compositing path remains
   in the steady-state frame loop.

2. **Virtual list as scene graph subtree**: `userspace/ui/widgets/virtual_list` creates and manages
   `SceneNode`s for visible items. Stable visible-range computation, recycling pool (reuse node slots),
   anchor-by-key, bounded mixed-height measurement caches. Scrolling = `PlaceOnly` invalidation on items.
   Consumes a minimal lazy-loading page provider (not a static array).

3. **Minimal lazy-loading provider**: `ItemProvider` trait with `len_hint()`, `get(index_range)`,
   `request_more(trigger)`. Deterministic viewport-based triggers (never timer-based). Bounded in-flight
   pages (max 1). Page arrival preserves scroll anchor-by-key and only invalidates affected measurement rows.
   (Full QuerySpec integration is TASK-0275.)

4. **Dual-panel blur**: sidebar glass panel + chat glass panel, both as scene graph `Group` nodes with
   `BackdropFilter` primitives. Composited in one GPU CB. Glass backdrop cache reused where panels overlap.

5. **Chat mockup**: 500+ messages with mixed heights (short "ok" to multi-paragraph), loaded via the
   lazy-loading provider. Prepend (scroll-up loads older messages) preserves anchor. Append (new message)
   does not destabilize visible range. The most troublesome virtual-list stress test.

6. **Virgl GPU blur rendering**: gpud backend virgl feature gate. When QEMU virtio-gpu has `virgl=on`,
   `BlurBackdrop` dispatches to a GPU separable gaussian shader instead of the CPU box-blur fallback.
   The RISC-V side sends the same CB; execution path is runtime-selected.

7. **Theme tokens v1**: `userspace/ui/theme` — roles/tokens schema and loader, light/dark modes and
   overrides, notification to dependents (signal-based), live switching via configd 2PC.

8. **120 Hz pacing proof**: p95 frame interval ≤ 8.3ms (120 Hz) under dual-panel blur + virtual list
   scroll load. Degradation order when budget is exceeded: blur radius → blur sample count → glass
   quality tier → frame rate. Without virgl, the CPU box-blur path targets p95 ≤ 16.7ms (60 Hz) as the
   realistic floor; the 120 Hz claim is validated against the virgl profile when it lands.

## Non-Goals

- Full QuerySpec integration (TASK-0275)
- Multiple concurrent providers
- Full design system (TASK-0073)
- virgl Venus/Vulkan backend (virgl GLES is sufficient)
- Kernel changes
- GPU-side SDF or text rendering (those remain CPU/atlas for now; blur is the priority GPU offload)

## Constraints / invariants (hard requirements)

- **Scene graph is sole rendering authority**: after Phase 1, no CPU compositing path remains in the
  steady-state frame loop. `write_rows`, `write_damage_rect`, `copy_scene_row`, `dark_glass_row`,
  `compute_shadow_row` are deleted.
- **120 Hz pacing floor** (with virgl): p95 frame interval ≤ 8.3ms under dual-panel blur + virtual list
  scroll. Without virgl: p95 ≤ 16.7ms (60 Hz). Degradation order: blur radius → blur sample count →
  glass quality tier → frame rate. Never allow unbounded queue growth or multi-second stalls.
- **Virgl gate**: `BlurBackdrop` GPU shader path is feature-gated. CPU fallback must remain functional
  and produce identical visual output (within 1-bit tolerance). Golden tests pass on both paths.
- **Deterministic virtualization**: given viewport/scroll position, visible range is stable.
  Prepend/append and width-bucket changes preserve deterministic anchor behavior.
- **Lazy-loading triggers**: viewport/index-based, never timer-based. At most 1 in-flight page request
  per provider.
- **Pretext cache reuse**: paragraph/run cache + line-layout cache split must be reused for virtual list
  row measurement. No redundant text shaping on scroll or page load.
- **Bounded memory**:
  - Scene graph: `MAX_NODES = 2048` (raised from 256 for chat mockup + panels + shell)
  - Recycled pool: cap recycled surfaces and cached row measurements
  - Provider: cap in-flight pages (1), cap loaded page count
  - Theme tokens: cap parsed tree depth and token sizes
- **Invalidation posture**:
  - Scroll = PlaceOnly (positions change, content doesn't)
  - New items = PaintOnly on the new range
  - Width-bucket change = remeasure affected rows only, preserve anchors
  - Unchanged state = Clean (subtree hash match → skip entire subtree)
- No `unwrap`/`expect`; no blanket `allow(dead_code)`.
- No debug logs in kernel.

## Red flags / decision points

- **RED (virgl dependency for 120 Hz)**: 120 Hz pacing with blur cannot be honestly claimed on the CPU
  box-blur path. The virgl profile must be functional and measured. If virgl integration proves blocked
  (QEMU version, host GL driver, RISC-V virgl support gap), document the blocker explicitly and target
  60 Hz as the proven ceiling with a plan for virgl follow-up.
- **YELLOW (hardening plan scope)**: Phases A1–A4 + C of the hardening plan are estimated at ~3h.
  If unexpected coupling with the bootstrap/visible path surfaces, scope down: execute A1 (remove CPU blur)
  and C (wire scene graph) first, defer A2–A4 wallpaper/SDF removal if needed.
- **YELLOW (config dependency)**: Live theme switching depends on `configd` and `/state/config` overrides
  being real; host tests must simulate this cleanly.
- **YELLOW (MAX_NODES)**: 2048 nodes may need arena/allocation strategy review for `no_std` OS path.
  If bump-allocator pressure is too high, reduce to 1024 and cap chat mockup at 300 messages for OS proof.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v5b_host/`:

- **Scene graph wiring**: CB generated from scene graph dirty set; no `write_rows`/`write_damage_rect` calls
  in frame path; golden frame output matches pre-hardening baseline (1-bit tolerance for blur differences).
- **Virtual list**: 1000 items + small viewport → stable visible range; scrolling by N viewports triggers
  bounded recycle events; prepend/append preserves deterministic anchor; width-bucket change remeasures only
  affected rows.
- **Lazy-loading provider**: scrolling over 500 items triggers deterministic page requests; only 1 in-flight
  at a time; placeholder→loaded replacement preserves anchor.
- **Chat mockup**: 500 messages, mixed heights (1-line to 8-line), prepend (scroll-up) loads older page and
  preserves anchor; anchor stable across 3 sequential page loads.
- **Dual-panel blur**: sidebar + chat panel composited in one CB; backdrop cache reused where panels overlap;
  `BackdropFilter` primitives on both Group nodes.
- **Theme**: load tokens (default + override); role-to-RGBA mapping stable; switching notifies dependents
  exactly once per commit.
- **Pacing**: 1000 scroll frames, p95 frame interval ≤ 8.3ms (virgl) or ≤ 16.7ms (CPU fallback);
  no frame > 24ms; degradation policy verifiable under overload injection.

### Proof (OS/QEMU) — gated

UART markers (order tolerant):

- `windowd: scene graph on`
- `windowd: gpu pipeline on`
- `gpud: virgl ready` (when virgl profile active; `gpud: cpu fallback` otherwise)
- `ui: virtual list on`
- `virtualize: mount(<n>)`
- `virtualize: recycle(<n>)`
- `virtualize: live scroll ok`
- `virtualize: page load ok`
- `virtualize: prepend anchor ok`
- `ui: 120hz pacing ok (dual blur + list)` (virgl profile)
- `ui: 60hz pacing ok (dual blur + list)` (CPU fallback profile)
- `uitheme: loaded (mode=light|dark)`
- `uitheme: switched (to=dark)`
- `SELFTEST: ui v5 virtualize ok`
- `SELFTEST: ui v5 theme ok`
- `SELFTEST: ui v5 scene graph ok`

### Visual proof — required

- Two blurred glass panels visible simultaneously on the shared proof surface (sidebar + chat panel)
- One panel contains a scrolling virtualized chat list with visible message content (Text primitives)
- Live scroll visibly changes the viewport while preserving anchors
- Theme switching visibly recolors both panels (not only a host snapshot)
- Cursor visible and responsive during scroll (no input starvation)

## Plan (small PRs)

1. **Hardening Phase A1 + C**: Remove CPU blur path; wire scene graph — `generate_commands()` from
   dirty nodes; GPU-only CB in `flush_pending_damage`. Delete `backdrop.rs`. Prove golden output matches.
2. **Hardening Phase A2–A4**: Replace CPU SDF/panel/wallpaper rendering with CB commands. Delete
   `shadow.rs`, `surface.rs`, `source.rs`, `scene.rs`. Prove no `vmo_write` in frame path.
3. **Extend scene graph**: raise `MAX_NODES` to 2048; add `batch_insert`, node recycling (`recycle` /
   `set_text_content` / `set_rect`); add `generate_commands` support for `Text`, `BackdropFilter`,
   `Group` with shadow primitives.
4. **Virtual list widget**: `userspace/ui/widgets/virtual_list/` — creates and manages `SceneNode`s
   for visible items; recycling pool; anchor-by-key; bounded mixed-height measurement caches; mounts
   into the SystemUI shell's scene graph.
5. **Minimal lazy-loading provider**: `ItemProvider` trait; viewport-based triggers; deterministic
   page tokens; integration with virtual list (provider → scene graph nodes).
6. **Chat mockup**: 500+ messages, mixed heights; prepend/append via provider; scroll anchor
   preservation; mounts as a second glass panel in the proof surface.
7. **Dual-panel blur**: second `Group` node with `BackdropFilter`; backdrop cache sharing; both panels
   in one CB.
8. **Theme tokens v1**: schema, loader, light/dark modes, live switching via configd 2PC.
9. **120 Hz pacing proof**: measure p50/p95/p99 frame intervals; degradation policy verification;
   golden + regression gates.
10. **Virgl GPU blur rendering**: gpud `virgl` feature gate; separable gaussian shader; QEMU
    `-device virtio-gpu-pci,virgl=on` profile; visual parity with CPU fallback (1-bit tolerance).
11. **Host tests + OS markers + docs**: `tests/ui_v5b_host/`; selftest-client markers; update
    `docs/dev/perf/` with virgl profile results.

## Touched paths (allowlist)

- `source/services/windowd/src/scene_graph.rs` (extend: `generate_commands`, `MAX_NODES`, batch ops, recycling)
- `source/services/windowd/src/systemui_shell.rs` (extend: dual-panel mount, chat panel mount)
- `source/services/windowd/src/compositor/runtime.rs` (rewrite: scene graph frame path, remove CPU compositing)
- `source/services/windowd/src/compositor/mod.rs` (remove dead module declarations)
- `source/services/windowd/src/compositor/backdrop.rs` (DELETE — CPU blur)
- `source/services/windowd/src/compositor/shadow.rs` (DELETE — CPU shadow)
- `source/services/windowd/src/compositor/surface.rs` (DELETE — CPU SDF)
- `source/services/windowd/src/compositor/source.rs` (DELETE — CPU wallpaper)
- `source/services/windowd/src/compositor/scene.rs` (DELETE — CPU row compositing)
- `source/drivers/gpud/src/backend.rs` (extend: virgl feature gate, GPU shader dispatch)
- `source/drivers/gpud/Cargo.toml` (add: virgl feature flag)
- `userspace/ui/widgets/virtual_list/` (new)
- `userspace/ui/theme/` (new)
- `schemas/ui.tokens.schema.json` (new)
- `tests/ui_v5b_host/` (new)
- `source/apps/selftest-client/` (markers)
- `tools/postflight-ui-v5b.sh` (delegates)
- `docs/dev/ui/collections/widgets/virtual-list.md`
- `docs/dev/ui/foundations/visual/theme.md`
- `docs/dev/ui/foundations/layout/layout-pipeline.md`
- `docs/dev/perf/PLATFORM-CLASS-UI-PERFORMANCE-OPTIMIZATIONS-QEMU-MATRIX.md` (update: 120 Hz + virgl targets)
