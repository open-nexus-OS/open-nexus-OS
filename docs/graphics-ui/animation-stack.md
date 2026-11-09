---
schema: 1
title: "Animation stack"
role: guide
subsystem: ui
sources: [userspace/ui/animation, source/services/windowd/src/compositor/runtime/transitions.rs, source/services/windowd/src/compositor/runtime/frame_pulse.rs, source/services/windowd/src/compositor/runtime/surface_motion.rs, source/services/windowd/src/motion_geometry.rs, source/services/app-host/src/probe/anim.rs, source/services/app-host/src/probe/motion_layers.rs, source/services/app-host/src/probe/scroll.rs, source/services/app-host/src/probe/pulse.rs, userspace/nexus-gfx/src/backend]
contracts: [ADR-0031, RFC-0059, RFC-0110, ADR-0076, RFC-0109, ADR-0097]
verified: 2026-11-05
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Animation stack

> **Purpose** — where an animation is computed today, who ticks it, and how its frames reach the GPU.
> **Owns** — `userspace/ui/animation/`, `source/services/windowd/src/compositor/runtime/transitions.rs`, `source/services/windowd/src/compositor/runtime/frame_pulse.rs`, `source/services/app-host/src/probe/anim.rs`, `source/services/app-host/src/probe/scroll.rs`
> **Contracts** — [ADR-0031](../adr/0031-three-layer-animation-architecture.md) (the three layers: engine, NexusGfx, GPU driver), [RFC-0059](../rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md), [RFC-0110](../rfcs/RFC-0110-frame-timing-the-shown-time-the-cadence-the-motion-for-its-frame.md) (the shown time), [ADR-0076](../adr/0076-the-app-scrolls-and-hovers-windowd-composes-the-declared-layers.md) (scroll in the app), [RFC-0109](../rfcs/RFC-0109-surface-layers-the-app-scrolls-and-hovers-the-compositor-composes.md) §motion layers + [ADR-0097](../adr/0097-windowd-moves-the-declared-layers-an-app-states-a-transition-once.md) (a `.transition` enter moved by windowd)
> **Invariants** — one physics crate (`animation`) for every runner. A window transition and a `.transition` enter on the desktop cost no app re-render: each rides the next present as a transform. Animations step to the time their frame is shown, not to the time they are computed.
> **Read next** — [nexus-gfx.md](nexus-gfx.md), [display-chain.md](display-chain.md), [../dev/ui/foundations/animation.md](../dev/ui/foundations/animation.md)

ADR-0031 explains the three layers. This page maps them to the code as it runs now.

## Layer 1 — the engine (`userspace/ui/animation/`)

| module | content |
|---|---|
| `timeline.rs` | `AnimationDriver`: active animations (at most 16), `tick(now_ns)`, reduced motion caps durations at 100 ms |
| `spring.rs` | `Spring`, the spring solved in closed form at any time (TASK-0354 P0: no step, a retarget keeps the velocity, `libm` for the same answer on every core); `SpringSim`, its stepped RK4 twin — the closed form is within half a pixel of it at 60 Hz (`spring_closed_form_tests`) |
| `keyframe.rs` | `KeyframeTrack` with linear and ease curves |
| `property.rs` | `AnimProp`, `SceneUpdate`, `Easing`, `SpringConfig` |
| `motion.rs` | the closed motion-token catalog the DSL's `.animate` / `.transition` / `.effect` validate against; durations and curves come from `nexus-theme-tokens` |
| `scroll.rs` | `ScrollMomentum`, the one scroll physics (wheel, fling, paging) |

Tests: `userspace/ui/animation/tests/` (keyframe, spring, timeline ordering).

## Who ticks it

| runner | what | clock | output |
|---|---|---|---|
| windowd (`compositor/runtime/transitions.rs`) | window open (fade + scale), close (fade), minimize (fly to the taskbar) | the display's acks (`source/services/windowd/src/frame_clock.rs`) | one `Command::LayerTransform` per window in the next present; the planner redraws the old and new area. No app re-render, no CPU recomposite. |
| windowd (`compositor/runtime/surface_motion.rs`, `motion_geometry.rs`) | a `.transition` enter the app declared as a motion layer (RFC-0109 §motion layers, TASK-0354 P2): the shell's panels, the launcher, the greeter's content | the display's acks — each frame evaluated for its predicted shown time | one `Command::LayerTransform` per moving layer in the next present; its pixels painted once by the app into its store, its regions, slices, washes and caret moved with it; a moving glass keeps its last blur. No app wake, no raster per frame. |
| app-host (`probe/anim.rs`) | DSL `.animate` / `.effect`, a press's dip held from TAP to RELEASE (95 % control, 88 % chrome circle, 92 % app icon — `press_kind.rs`), and a `.transition` the compositor cannot draw (an effect token, a window's frosted base, a full store) | windowd's frame pulse | a per-node transform the CPU painter (`nexus-scene-raster`) applies in the app's own frame |
| app-host (`probe/anim/interaction.rs`, `interaction_layer.rs`) | a control's hover and press layers (RFC-0116: `fillHover` / `fillActive`, the on-fill pair over a tint fill), faded by opacity — hover `swift`, press `instant` | the frame pulse, while a layer fades | washes windowd composes (`OP_SURFACE_MOTION`, RFC-0109): the opacity per frame, never a raster; hover never changes a control's size |
| app-host (`probe/scroll.rs`) | every `.scroll(...)` container | the frame pulse | layer offsets windowd composes (ADR-0076) |
| windowd (`compositor/runtime/cursor.rs`, `wake_clock.rs`) | the wait ring on the pointer while a launch has not surfaced | the loop's one-shot wake, one sprite every 90 ms — never a present's completion, which returns in microseconds on a display without a vertical blank | one cursor-shape select per step (and the pointer rect presented for a software pointer); between steps nothing |

A motion layer (`probe/motion_layers.rs`) is declared in `anim_sync` instead of starting the node's keyframes: the app places the subtree's paint extent in its store, paints it once at rest (`apphost: motion layer (id= token= w= h= paint_us=)`, once per layer), paints the base without it and states the motion with the structure — token, duration, the offset and opacity it starts from, what it claims. windowd evaluates the token's design curve exactly (the same `Easing::Curve` the app's track uses) and scales a growing entrance about the layer's centre (`Command::LayerTransform` with a 1/10000 scale and an origin); it starts the motion with the first frame that draws its id (`windowd: motion layer (owner= id= …)`, once) and owes frames until it rests; the same id re-declared continues. The layer stays while the node is mounted: a change inside it paints its store rows (never the base's — the layout diff splits the rows, `layout_diff::changed_rows`), a close is its absence. A modal's or a toast's layer (`.overlay(modal|transient)`) is drawn in front — after the windows, the bar and the panels (`Pass::DesktopFront`); a root that is one flat fill over the surface (a scrim) becomes the layer's backdrop, one patch texel windowd spreads and fades with the layer (`apphost: motion layer (… front= backdrop= …)`) — the first two layers have a patch for it (the first and the last patch are the hover and press layers' colours); a third paints its scrim into its store rows.

The frame pulse (`compositor/runtime/frame_pulse.rs`): an animating client sends `OP_SURFACE_FRAME_REQ` once; windowd answers with one `OP_SURFACE_FRAME` after its next composited frame. The answer names when the client's next frame is shown and the time between frames on the glass. Nothing animates on a busy loop. In the app the pulse lands in `probe/pulse.rs`: the scroll physics, the DSL animations and the fades of the hover and press layers advance to that shown time, and the motion chain's hop is kept — `apphost: hops motion (pulse>sent= ticks= paint_us= rows= px= tokens=)` says per motion episode how long a pulse took to its present, how often the app woke, what each frame's raster cost and how many rows it painted, how much damage each present declared, and which motion tokens ran (RFC-0108 §hops, `nexus_ux_budgets::chains`).

## Layers 2 and 3 — NexusGfx and the backend

- windowd's present is a `nexus_gfx` command buffer ([nexus-gfx.md](nexus-gfx.md)). gpud's frame planner turns its layer set into GPU passes ([display-chain.md](display-chain.md)).
- `GfxBackend` is in `userspace/nexus-gfx/src/backend/traits.rs` (there is no separate `gfx-backend` crate). It has two implementations: `CpuMockBackend` (`userspace/nexus-gfx/src/backend/cpu_mock.rs`, the host golden reference) and `VirtioGpuBackend` (`source/drivers/gpud/src/backend/mod.rs`). Both execute through the one software rasterizer, `userspace/nexus-gfx/src/raster/`.
- The board's GPU does not go through `GfxBackend`. It uses the v2 device contract (`userspace/nexus-gfx/src/backend/v2/`, RFC-0101) and the frame planner (`userspace/nexus-gfx/src/frame/`).

## RISC-V details still in the code

| where | what |
|---|---|
| `userspace/ui/sdf/src/fixed.rs` | `circle_sd`: fixed-point circle SDF, integers only |
| `userspace/nexus-gfx/src/raster/blend.rs` | `div255` as multiply + shift |
| `.cargo/config.toml` | `target-feature=+zbb` for the OS targets |

## Proof

- `windowd: transition open` (proof manifest, `source/apps/selftest-client/proof-manifest/markers/ui.toml`) — an app window's open transition began; the usb-visible and lowmem scenarios expect it.
- `scripts/qemu-test.sh` keeps the TASK-0062 guard: `SELFTEST: ui v5 transition ok` counts only after the gpud and animation bridge markers (`uiruntime: on`, `uianim: timeline on`, `windowd: implicit transitions on`, `windowd: live transition ok`).
- `cargo test -p animation`.
