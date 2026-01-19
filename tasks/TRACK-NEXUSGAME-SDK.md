---
title: TRACK NexusGame SDK (games): contracts + phased roadmap (capability-first, deterministic, NexusGfx-backed)
status: Living
owner: @ui @runtime
created: 2026-01-18
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - NexusGfx SDK track (render/compute): tasks/TRACK-NEXUSGFX-SDK.md
  - NexusMedia SDK track (audio/video/image): tasks/TRACK-NEXUSMEDIA-SDK.md
  - NexusNet SDK track (cloud + DSoftBus): tasks/TRACK-NEXUSNET-SDK.md
  - Drivers & accelerators foundations: tasks/TRACK-DRIVERS-ACCELERATORS.md
  - Arcade (reference microgames: Breakout/Asteroids/Snake): tasks/TRACK-ARCADE-APP.md
  - Zero-copy VMOs (data plane): tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - QoS/timers (soft real-time): tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md
  - Deterministic parallelism policy: tasks/TASK-0276-parallelism-v1-deterministic-threadpools-policy-contract.md
  - Input bring-up direction (OS): tasks/TASK-0253-input-v1_0b-os-hidrawd-touchd-inputd-ime-hooks-selftests.md
---

## Goal (track-level)

Deliver a first-party **NexusGame SDK** that enables:

- small 2D/3D indie games,
- UI-heavy interactive scenes,
- “pro realtime” interactive tools (CAD viewports, editors) sharing the same render/input/timing primitives,

while staying aligned with Open Nexus OS constraints:

- **capability-first security** (no ambient GPU/audio/input device access),
- **zero-copy data plane** (VMO/filebuffer for bulk assets and frame buffers),
- **soft real-time** pacing (deadlines/QoS/backpressure),
- **deterministic tooling** (host-first tests; stable perf/scene goldens),
- **small TCB** (device validation in userland; kernel stays minimal).

## Scope boundaries (anti-drift)

- This is **not** “build a full game engine immediately”.
- We avoid importing large compatibility stacks as the core (POSIX/Vulkan/OpenGL clones).
- We prefer **NexusGfx + NexusMedia** as the stable substrate; game ergonomics layer on top.

## Shared primitives (foundation)

The game SDK is primarily a composition of shared primitives:

- **Graphics**: NexusGfx device/queue/buffer/image/pipeline + explicit sync.
- **Audio**: NexusMedia/NexusAudio stream model (audiod authority).
- **Input**: stable event model (bounded queues, deterministic ordering) with OS owners/services.
- **Timing**: frame clock + deadlines; no reliance on wall-clock jitter for correctness.
- **Assets**: content-addressed or hash-checked bundles, with bounded decode and deterministic loaders.

## Reference inspirations (design, not compatibility)

These are good “API-shape” inspirations:

- **Bevy**: ECS + plugin composition (Rust-native ergonomics).
- **SDL2/raylib**: simple, approachable surface for small games.
- **Godot**: scene graph patterns (useful for tooling), but we do not copy the runtime model 1:1.

## Authority model

- **Graphics device access** is mediated by the OS device broker/driver services (future real GPU),
  but the SDK presents a stable “Metal-like” API surface (see NexusGfx track).
- **Audio** is mediated by `audiod` (single authority).
- **Input** is mediated by input services (`inputd` etc.); apps consume a safe event stream and
  do not touch device nodes directly.
- **Policy** decisions remain centralized (policyd/permsd) and are not reimplemented in game libraries.

## Phase map (what “done” means by phase)

- **Phase 0 (host-first ergonomics)**
  - a minimal game loop helper exists (frame tick + input queue + render submit),
  - deterministic “hello game” scenes with goldens exist (2D sprites/text; optional 3D triangles),
  - optional ECS crate can exist, but is not required for v0.

- **Phase 1 (OS wiring)**
  - input events are real on OS/QEMU (bounded), with deterministic selftests/markers,
  - NexusGfx present integration provides stable pacing primitives and perf hooks.

- **Phase 2 (pro games/tools)**
  - asset pipelines, shader artifacts (offline-first), performance regression gates,
  - real GPU backends behind the same SDK contracts.

## Candidate subtasks (to be extracted into real tasks)

- **CAND-GAME-000: NexusGame API v0 (loop + input + render glue)**
  - keep surface small; no physics requirement
  - proof: host goldens + deterministic input playback fixtures

- **CAND-GAME-010: Deterministic input replay format v0**
  - record/replay bounded input streams for tests and perf gates

- **CAND-GAME-020: Optional ECS v0**
  - inspiration: Bevy-style ergonomics; keep minimal and deterministic

## Extraction rules

A candidate becomes a real `TASK-XXXX` only when it:

- has deterministic host proof (goldens and/or bounded perf traces),
- names its authority boundaries (no competing input/audio/gfx owners),
- is feature-gated and does not expand default OS dependency graphs without need.

## Capability names (v0 catalog; stable strings)

These are policy identifiers used by `policyd` and enforcement adapters. They must remain stable strings.

### Input

- `input.read` (receive input events for the focused app/window)
- `input.capture.global` (global shortcuts / background listeners; likely system-only)

### Game / realtime execution

- `realtime.perfburst` (request PerfBurst-style scheduling hints; privileged/system-only by default)

### Graphics device access (via NexusGfx)

Prefer to reuse/align with NexusGfx naming once it exists; initial placeholders:

- `gfx.device.use` (ability to create device/queue)
- `gfx.present` (present frames; usually implied by window ownership)
- `gfx.compute` (compute dispatch; gated)
