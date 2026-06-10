---
title: TASK-0062 UI v5a: Deterministic Animation + NexusGfx 2D Pipeline + GPU Driver Contract
status: Done
owner: @ui @runtime
created: 2025-12-23
updated: 2026-06-10 (TASK-0062 closed; RFC-0059 Complete; all phases proven; CHANGELOG synced)
depends-on: [TASK-0059]
follow-up-tasks: [TASK-0063, TASK-0064]
links:
  - **RFC (SSOT contract)**: docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md
  - Compositor baseline: tasks/TASK-0059-ui-v3b-clip-scroll-effects-ime-textinput.md
  - Gfx track: tasks/TRACK-NEXUSGFX-SDK.md
  - Driver track: tasks/TRACK-DRIVERS-ACCELERATORS.md
  - Gfx architecture: docs/architecture/nexusgfx-command-and-pass-model.md
  - Device/MMIO model: tasks/TASK-0010-device-mmio-access-model.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

TASK-0059 delivered a CPU compositor (`compositor/` — 18 files: runtime, surface,
backdrop, filter, shadow, scene, cache, tile_map, types, sdf, primitives, damage, blur, source,
font, cursor, path_cache, tests). The compositor is "immediate": no animation timeline, no GPU offload.

RFC-0059 defines the architecture. This task implements all phases.

## Completed Phases

- Phase 0 (Animation Engine): ✅ — `userspace/ui/animation/`
- Phase 1 (NexusGfx SDK Core): ✅ — Device, Queue, CommandBuffer, Fence, GfxError
- Phase 2 (GfxBackend + CpuMockBackend): ✅ — trait + CPU reference
- Phase 3 (gpud + virtio-gpu MMIO): ✅ — probe, virtqueue, ATTACH_BACKING, scanout
- Phase 4 (windowd integration): ✅ — AnimationDriver in runtime, implicit transitions
- Phase 5 (NexusGfx module structure): ✅ — 10-module tree
- Phase 6a (CommandBuffer wire format): ✅ — serialize/deserialize, round-trip tests
- Phase 6b (Reactive gpud IPC): ✅ — Wait::Blocking

## Current Phase: 6c — GPU-first Rendering Pipeline

### Goal

Replace the dual-path architecture (CPU compositor + parallel GPU metadata path) with a single GPU-first pipeline:

- windowd builds ONE `CommandBuffer` describing the entire frame
- ONE IPC to gpud (not per damage-rect)
- gpud renders the complete frame (CpuMockBackend on host, VirtioGpuBackend on OS)
- No `vmo_write()` from windowd, no CPU compositing in windowd
- gpud writes directly into the framebuffer VMO (ATTACH_BACKING zero-copy)

### Stop conditions

#### Host
```bash
cargo test -p nexus-gfx -p gpud -p windowd
```

New tests:
- `nexus-gfx`: Command round-trip for BlitSurface, FillSdfRoundedRect, BlurBackdrop
- `nexus-gfx`: CpuMockBackend golden output matches reference for known inputs
- `windowd`: build_frame_commands produces valid CommittedBuffer
- `gpud`: VirtioGpuBackend renders BlitSurface, FillSdfRoundedRect, BlurBackdrop

#### QEMU
```text
gpud: ready              gpud: cb render ok
windowd: cb submit ok    windowd: single-ipc frame ok
SELFTEST: ui v5 gpu pipeline ok
```

## Phase 6c -> 7 Hard Closure Contract (execution SSOT)

### Cross-track mandatory dependency (kernel timer package)
- The kernel timer capability work package from
  `docs/dev/perf/KERNEL-TIMER-CAPABILITY-ANALYSIS.md` Phase 2 is required for closure.
- Scope baseline: timer syscall/capability/queue/IRQ delivery integration.
- Planned effort: **6-8 engineer-days**.
- Phase 7 cannot close before this dependency is merged and proven.

### Phase 6c must-have (implementation)
- `windowd` primary frame path = one committed command stream per frame.
- `gpud` primary frame path = execute command stream and present once per frame.
- No primary fallback to per-row CPU `vmo_write()` compositing in steady-state.

### Phase 6c must-have (proof)
- Host: command execution tests for all active command tags.
- QEMU: marker ladder confirms command-stream submit -> render -> present.
- Negative proof: malformed command stream is rejected deterministically.

### Phase 6d must-have (implementation)
- Asynchronous fence state machine (not pre-signaled).
- Double-buffer (or better) with bounded in-flight frames (target max: 2).
- Completion correlation (`present_id`/equivalent sequence ID).

### Phase 6d must-have (proof)
- Host: fence order and timeout tests.
- QEMU: delayed completion does not deadlock; forward progress remains bounded.
- Backpressure gate: queue/in-flight counters stay within configured limits.

### Phase 6e must-have (implementation)
- Fixed-point hot path for blend/SDF/blur in backend execution path.
- Deterministic quality degrade policy for overload (bounded and explicit).

### Phase 6e must-have (proof)
- Host comparator shows bounded output delta vs reference.
- Microbench documents hot-loop speedup.
- Long-run test confirms no unbounded heap growth in render hot path.

### Phase 7 must-have (implementation + proof)
- Golden suite for blur/shadow/rounded/text/cursor composition.
- Regression gates for p50/p95/p99, input latency under load, memory stability.
- QEMU profile matrix captured and reproducible (including `virgl` target profile when enabled).
- Kernel timer capability is active in the frame pacing path (not optional for closure).
- Present completion feedback is active and correlated in the frame pacing path.
- Final pacing policy is timer + present completion closure with bounded in-flight frames.
- Timer package dependency (6-8d) is completed and linked proof artifacts are present.

### Explicit fail conditions (task stays In Progress)
- `submit()` remains validate-only/no-op for rendering.
- Fence path remains always-signaled.
- "Smoothness" claimed without metric gates or with marker-only evidence.
- Phase 7 claimed without kernel timer capability in the active pacing path.
- Phase 7 claimed without present completion feedback correlation in the active pacing path.

### Architecture invariant

```text
windowd (Producer)                    gpud (Consumer/Renderer)
  build_frame_commands()                recv(Wait::Blocking)
  → CommittedBuffer                     → backend.submit(cb)
  → IPC (ONE per frame)                 → render into VMO
  NO vmo_write()                        → TRANSFER_TO_HOST_2D (once)
  NO CPU compositing                    → RESOURCE_FLUSH (once)
                                        → fence.signal()
```

## Pending Phases

### Phase 6d — Async Fence + Double-Buffer Pipelining
- `Fence` with `wait()` and `signal()`
- Two framebuffer VMOs (ping-pong)
- windowd builds frame N+1 while gpud renders frame N
- Completion correlation (`present_id` or equivalent) is mandatory
- In-flight frame count must stay bounded and observable

### Phase 6e — RISC-V Fixed-Point Rendering
- Port fixed-point SDF from `fixed_sdf.rs` into backend
- `(a*b*257)>>16` blend in backend
- `+zbb` target-feature
- Fixed-point output parity threshold must be defined and tested

### Phase 7 — Golden Tests + Perf Regression Gates
- Pixel-golden: CpuMockBackend == VirtioGpuBackend
- Frame-time histogram
- Zero heap growth proof
- Profile-class pacing gates (light/medium/heavy scenes)
- Input-latency-under-load gate
- QEMU reachability matrix with explicit "required vs stretch" targets
- Phase does not close until timer + present completion pacing closure is proven

## Performance target reference

Use `docs/dev/perf/PLATFORM-CLASS-UI-PERFORMANCE-OPTIMIZATIONS-QEMU-MATRIX.md` as the performance target matrix for phase 7 sign-off.

## Touched paths (allowlist)

- `userspace/nexus-gfx/src/command/buffer.rs` (new commands + serde)
- `userspace/nexus-gfx/src/backend/cpu_mock.rs` (real SW rasterizer)
- `userspace/nexus-gfx/src/core/fence.rs` (async Fence)
- `userspace/nexus-gfx/tests/` (command + golden tests)
- `source/services/windowd/src/compositor/runtime.rs` (build_frame_commands, single IPC)
- `source/services/windowd/src/compositor/mod.rs` (IPC loop)
- `source/drivers/gpud/src/service.rs` (full CB handler)
- `source/drivers/gpud/src/backend.rs` (VirtioGpuBackend rendering)
- `docs/rfcs/RFC-0059-*.md` (updated)
- `tasks/TASK-0062-*.md` (this file)
