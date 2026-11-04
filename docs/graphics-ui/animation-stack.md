---
schema: 1
title: "Animation stack"
role: guide
subsystem: ui
sources: [userspace/ui/animation, source/services/windowd/src/compositor/runtime/transitions.rs, source/services/windowd/src/compositor/runtime/frame_pulse.rs, source/services/app-host/src/probe/anim.rs, source/services/app-host/src/probe/scroll.rs, source/services/app-host/src/probe/pulse.rs, userspace/nexus-gfx/src/backend]
contracts: [ADR-0031, RFC-0059, RFC-0110, ADR-0076]
verified: 2026-11-03
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Animation stack

> **Purpose** — where an animation is computed today, who ticks it, and how its frames reach the GPU.
> **Owns** — `userspace/ui/animation/`, `source/services/windowd/src/compositor/runtime/transitions.rs`, `source/services/windowd/src/compositor/runtime/frame_pulse.rs`, `source/services/app-host/src/probe/anim.rs`, `source/services/app-host/src/probe/scroll.rs`
> **Contracts** — [ADR-0031](../adr/0031-three-layer-animation-architecture.md) (the three layers: engine, NexusGfx, GPU driver), [RFC-0059](../rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md), [RFC-0110](../rfcs/RFC-0110-frame-timing-the-shown-time-the-cadence-the-motion-for-its-frame.md) (the shown time), [ADR-0076](../adr/0076-the-app-scrolls-and-hovers-windowd-composes-the-declared-layers.md) (scroll in the app)
> **Invariants** — one physics crate (`animation`) for every runner. A window transition costs no app re-render: it rides the next present as a transform. Animations step to the time their frame is shown, not to the time they are computed.
> **Read next** — [nexus-gfx.md](nexus-gfx.md), [display-chain.md](display-chain.md), [../dev/ui/foundations/animation.md](../dev/ui/foundations/animation.md)

ADR-0031 explains the three layers. This page maps them to the code as it runs now.

## Layer 1 — the engine (`userspace/ui/animation/`)

| module | content |
|---|---|
| `timeline.rs` | `AnimationDriver`: active animations (at most 16), `tick(now_ns)`, reduced motion caps durations at 100 ms |
| `spring.rs` | `SpringSim`, fixed-timestep RK4 — the same dt gives the same position on x86_64 and riscv64 |
| `keyframe.rs` | `KeyframeTrack` with linear and ease curves |
| `property.rs` | `AnimProp`, `SceneUpdate`, `Easing`, `SpringConfig` |
| `motion.rs` | the closed motion-token catalog the DSL's `.animate` / `.transition` / `.effect` validate against; durations and curves come from `nexus-theme-tokens` |
| `scroll.rs` | `ScrollMomentum`, the one scroll physics (wheel, fling, paging) |

Tests: `userspace/ui/animation/tests/` (keyframe, spring, timeline ordering).

## Who ticks it

| runner | what | clock | output |
|---|---|---|---|
| windowd (`compositor/runtime/transitions.rs`) | window open (fade + scale), close (fade), minimize (fly to the taskbar) | the display's acks (`source/services/windowd/src/frame_clock.rs`) | one `Command::LayerTransform` per window in the next present; the planner redraws the old and new area. No app re-render, no CPU recomposite. |
| app-host (`probe/anim.rs`) | DSL `.animate` / `.transition` / `.effect` on a node | windowd's frame pulse | a per-node transform the CPU painter (`nexus-scene-raster`) applies in the app's own frame |
| app-host (`probe/scroll.rs`) | every `.scroll(...)` container | the frame pulse | layer offsets windowd composes (ADR-0076) |

The frame pulse (`compositor/runtime/frame_pulse.rs`): an animating client sends `OP_SURFACE_FRAME_REQ` once; windowd answers with one `OP_SURFACE_FRAME` after its next composited frame. The answer names when the client's next frame is shown and the time between frames on the glass. Nothing animates on a busy loop. In the app the pulse lands in `probe/pulse.rs`: the scroll physics and the DSL animations advance to that shown time, and the motion chain's hop is kept — `apphost: hops motion (pulse>sent= ticks=)` says per motion episode how long a pulse took to its present and how often the app woke (RFC-0108 §hops, `nexus_ux_budgets::chains`).

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
