---
title: TRACK NexusGfx SDK (Metal-like, apps + games): contracts + phased roadmap (2D/3D/compute/audio/video/cad)
status: Living
owner: @ui @runtime
created: 2025-12-26
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - NexusMedia SDK track (audio/video/image): tasks/TRACK-NEXUSMEDIA-SDK.md
  - NexusGame SDK track (games): tasks/TRACK-NEXUSGAME-SDK.md
  - NexusNet SDK track (cloud + DSoftBus): tasks/TRACK-NEXUSNET-SDK.md
  - Drivers & accelerators foundations: tasks/TRACK-DRIVERS-ACCELERATORS.md
  - NexusFrame (Pixelmator-class editor; reference workload): tasks/TRACK-NEXUSFRAME.md
  - Device/MMIO access model (gate): tasks/TASK-0010-device-mmio-access-model.md
  - Zero-copy VMOs (gate): tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - QoS/timers (soft real-time spine): tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md
  - Perf tracing + gates: tasks/TASK-0143-perf-v1a-perfd-frame-trace-metrics.md
  - Perf hooks + HUD: tasks/TASK-0144-perf-v1b-instrumentation-hud-nx-perf.md
  - Perf regression gates: tasks/TASK-0145-perf-v1c-deterministic-gates-scenes.md
---

## Goal (track-level)

Deliver a first-party, explicit, Metal-like SDK (“NexusGfx”) that can support:

- **Apps**: SystemUI, editors, PDF/markdown viewers, CAD, video editing, audio DAW UIs.
- **Games**: simple 3D titles (e.g., pinball/mario64-class), 2D/3D indie games, UI-heavy scenes.

with these system properties:

- **capability-first security** (no ambient global device),
- **zero-copy data plane** (VMO/filebuffer end-to-end),
- **soft real-time** pacing primitives (deadlines/QoS/backpressure),
- **deterministic tooling** (goldens, perf gates, reproducible shader/pipeline artifacts),
- **small trusted computing base** (validation in userland; kernel stays minimal).

## Non-Goals

- This file is **not** an implementation task.
- No QEMU markers are required for this track document itself.
- No kernel changes are defined here (kernel work gets its own tasks).
- POSIX/Vulkan compatibility is not a goal; compatibility shims (if any) are future, optional, and must not drive the core design.

## Contracts (stable interfaces to design around)

These are cross-cutting and must remain stable as the SDK evolves:

- **Buffers/images**: VMO/filebuffer descriptors, RO sealing, slices, budgets.  
  Source: `tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md`
- **Sync**: timeline fences + waitsets + deadlines (avoid busy-wait).  
  Source: `tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md`
- **Device access**: cap-gated device broker model (MMIO/IRQ/DMA handles).  
  Source: `tasks/TASK-0010-device-mmio-access-model.md`
- **Present**: compositor presents VMO-backed surfaces using fences + vsync spine.  
  Sources: `tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md`, `tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md`
- **Policy**: `policyd` decides; kernel enforces rights on held caps; audits are required for sensitive operations.  
  Sources: `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md`, `tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md`
- **Perf gates**: regression gates must be deterministic and enforced via host tests first.  
  Sources: `tasks/TASK-0143`/`0144`/`0145`

## Related tracks (intentional split, shared primitives)

- **Media/audio/video/images**: `tasks/TRACK-NEXUSMEDIA-SDK.md`
- **Games** (input/timing glue on top of NexusGfx + NexusMedia): `tasks/TRACK-NEXUSGAME-SDK.md`

## Gates (RED / YELLOW / GREEN)

- **RED (blocking)**:
  - **Real device access**: no real GPU/NPU/ISP backend without `TASK-0010`.
  - **Zero-copy correctness**: SDK relies on VMO semantics and transfer rules (`TASK-0031`).
- **YELLOW (risky / drift-prone)**:
  - **Shader toolchain**: on-device JIT can explode TCB; prefer deterministic offline compilation and signed artifacts by default.
  - **ISA/firmware constraints**: real GPU backends may require firmware; licensing and supply-chain gating must be explicit.
  - **Timing stability**: QEMU timing is not a stable perf oracle; CI gates must be host-first.
- **GREEN (confirmed direction)**:
  - Explicit sync + explicit resource lifetime fits the OS security model and service isolation.

## Phase map (what “done” means by phase)

- **Phase 0 (SDK skeleton + CPU backend)**
  - API surface exists with stable types and error semantics.
  - CPU backend supports: 2D blit/composite + basic textured triangles (optional).
  - Validation is bounded and deterministic.
  - Host goldens + perf traces exist.

- **Phase 1 (3D basics + tooling + safety)**
  - 3D pipeline basics (vertex/index buffers, textures, samplers, render passes).
  - Compute basics (dispatch, storage buffers).
  - Deterministic shader/pipeline artifact flow (offline compile + signing).
  - Robust budgets/backpressure + device reset/fault containment semantics (even on CPU backend).

- **Phase 2 (pro features, power-aware, and real device backends)**
  - Real GPU backend(s) can implement the same contracts behind the SDK.
  - Frame pacing integration with compositor and perf HUD.
  - CAD/video/audio workloads: large buffers, timeline scheduling, batching, low jitter.

## Backlog (Candidate Subtasks)

### SDK core

- **CAND-GFX-000: NexusGfx API v0 (types + errors + device/queue model)**  
  - explicit `Device`, `Queue`, `CommandBuffer`, `Fence`, `Buffer`, `Image`, `Sampler`, `Pipeline`  
  - deterministic error model (no stringly-typed failures)

- **CAND-GFX-001: Validation layer v0 (bounded, deterministic)**  
  - validate command buffers and resource states before submit  
  - deterministic “why denied” diagnostics (bounded)

- **CAND-GFX-002: Shader/IR toolchain v0 (offline-first, signed artifacts)**  
  - deterministic compilation outputs, stable IDs, signed shader blobs  
  - on-device compilation off by default

### Rendering features

- **CAND-GFX-010: 2D pipeline v1 (sprites, text, paths subset)**  
  - enables UI acceleration and simple games (pinball-class) even without real GPU

- **CAND-GFX-020: 3D baseline v1 (triangles, depth, basic materials)**  
  - enough for mario64-class scenes and simple CAD navigation

### Present + UI integration

- **CAND-GFX-030: windowd present integration (fences + pacing)**  
  - integrate perfd frame ticks, dropped frame accounting, and budget lines

### Pro workloads

- **CAND-GFX-040: CAD v1 primitives (instancing, large meshes, selection buffers)** (host-first proofs)
- **CAND-GFX-050: Video editing substrate (timelines, decode/compose hooks)** (ties to VPU track)
- **CAND-GFX-060: Audio DAW UI substrate (low jitter UI + audio sync points)** (ties to audio track)

## Notes: CAD (CPU vs GPU) — how we stay “state of the art” without breaking determinism

Modern CAD is not “CPU-only”. The common split is:

- **GPU (Phase 0/1: “no big problems”)**:
  - viewport rendering (lines/triangles, instancing, depth, culling, materials),
  - selection/picking (ID-buffer, depth picking, highlights),
  - overlays (gizmos, measurements, snapping guides, text/labels),
  - smooth navigation for large assemblies via LOD/streaming-friendly resource models.

- **CPU (authoritative geometry, Phase 0/1)**:
  - robust B-Rep kernels, boolean operations, topology edits,
  - constraint solving (sketch/assembly),
  - high-quality meshing and exactness-critical algorithms.

Why this split matches NexusGfx:

- The SDK can be “GPU-first” for the **interactive surface** (viewport + picking) while keeping a smaller,
  safer TCB for “authoritative geometry”.
- Deterministic proofs stay realistic:
  - we can gate CAD view correctness via deterministic render/picking goldens,
  - while keeping CPU geometry as the source of truth for exact operations (device-independent).

Phase 2+ can add optional GPU compute accelerations (meshing, acceleration structures) behind explicit
feature flags and deterministic test rules.

## Extraction rules (how candidates become real tasks)

- A candidate becomes a real `TASK-XXXX` only when:
  - it is implementable under current constraints (or explicitly creates prerequisite tasks),
  - it has proof (deterministic host tests and/or QEMU markers where valid),
  - it documents “minimal v1” vs “future deluxe” explicitly to avoid drift.
- When extracted:
  - this entry is replaced with `Status: extracted → TASK-XXXX` and a link,
  - all detailed requirements live in the real task, not here.
