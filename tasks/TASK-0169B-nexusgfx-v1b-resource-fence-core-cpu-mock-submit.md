---
title: TASK-0169B NexusGfx v1b (host-first): resource/fence core + CPU mock submit + deterministic validation/tests
status: Draft
owner: @ui @runtime
created: 2026-04-10
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - NexusGfx track: tasks/TRACK-NEXUSGFX-SDK.md
  - Renderer abstraction host slice: tasks/TASK-0169-renderer-abstraction-v1a-host-sceneir-cpu2d-goldens.md
  - Renderer abstraction OS wiring: tasks/TASK-0170-renderer-abstraction-v1b-os-windowd-wiring-textshape-perf-markers.md
  - DriverKit core contracts: tasks/TASK-0280-driverkit-v1-core-contracts-queues-fences-buffers.md
  - Zero-copy VMO plumbing: tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - QoS/timers contract: tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md
  - Gfx compute/executor model: docs/architecture/nexusgfx-compute-and-executor-model.md
  - Gfx resource model: docs/architecture/nexusgfx-resource-model.md
  - Gfx sync/lifetime model: docs/architecture/nexusgfx-sync-and-lifetime.md
  - Gfx capability matrix: docs/architecture/nexusgfx-capability-matrix.md
---

## Context

`TASK-0169` locks the host-first Scene-IR and backend abstraction, but it intentionally stops short of defining the
portable resource, queue, and fence vocabulary that later CPU/GPU/compute executors must share.

Without this slice, `windowd`, games, editors, and future compute consumers are likely to drift into ad-hoc buffer and
completion semantics before `NexusGfx` becomes real.

This task creates the first small but concrete `NexusGfx` substrate behind the existing renderer direction:

- typed resources,
- explicit queue/submit flow,
- bounded timeline-fence-like completion,
- and a deterministic CPU mock submit path usable in host tests.

## Goal

Deliver a host-first `NexusGfx` core that defines:

1. stable resource types:
   - `DeviceId`, `QueueId`, `BufferId`, `ImageId`, `FenceId`,
   - typed descriptors for buffer/image creation and import,
   - explicit size/stride/format/usage limits;
2. a bounded submit model:
   - `Queue::submit(...)`,
   - deterministic in-flight limits and backpressure,
   - completion via a minimal fence/timeline model with deadlines;
3. a CPU mock executor:
   - no real GPU work,
   - deterministic completion path for host tests,
   - stats/error surfaces suitable for later `windowd` and app integration;
4. host proofs:
   - `test_reject_*` for invalid descriptors/usages,
   - bounded queue/fence behavior tests,
   - stable error mapping.

## Non-Goals

- Real GPU hardware access.
- Shader/kernel compilation.
- A full graphics API surface (`Pipeline`, `Sampler`, render passes) beyond what is necessary to lock resource/sync shape.
- Replacing `TASK-0169`; this task extends it structurally.

## Constraints / invariants (hard requirements)

- Deterministic behavior and deterministic host proofs.
- Hard caps on resource sizes, queue depth, and in-flight submissions.
- No fake success: mock submit may only report completion after the modeled submit/completion path actually runs.
- No CPU-only assumptions in the API shape; imports/exports must remain compatible with future VMO/filebuffer backing.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Security considerations

### Threat model

- **Descriptor abuse**: malformed sizes/strides/usages used to trigger overflow or undefined state.
- **Queue exhaustion**: unbounded submit or wait behavior causing denial of service.
- **State confusion**: stale or forged handles causing use-after-free style bugs in later backends.

### Security invariants (MUST hold)

- All resource descriptors are validated before creation/import.
- Queue depth, wait counts, and deadlines are bounded.
- Handles are strongly typed and not stringly-typed.
- Failure reasons are deterministic and bounded.

### DON'T DO

- DON'T accept unbounded dimensions, strides, or queue depth.
- DON'T expose backend-private state through public errors.
- DON'T add a second buffer/fence vocabulary beside DriverKit/NexusGfx.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- Deterministic tests prove:
  - valid resource creation/import succeeds within bounds,
  - invalid size/stride/format/usage combinations are rejected,
  - queue backpressure is stable and bounded,
  - fence wait/complete semantics are deterministic,
  - CPU mock submit produces stable completion ordering.

### Proof (OS/QEMU) — not required

- This task is intentionally host-first.
- Any OS/QEMU marker work belongs in follow-up `TASK-0170B` or later backend integration tasks.

## Touched paths (allowlist)

- `userspace/libs/renderer/` or `userspace/libs/nexusgfx/` (resource/sync core)
- `tests/renderer_v1_host/` or `tests/nexusgfx_host/`
- `docs/architecture/nexusgfx-resource-model.md` (only if contract clarifications are required)
- `docs/architecture/nexusgfx-sync-and-lifetime.md` (only if contract clarifications are required)
- `tasks/TRACK-NEXUSGFX-SDK.md`

## Plan (small PRs)

1. Add typed resource/fence IDs and descriptors with bounded validation.
2. Add queue/submit/fence core with deterministic backpressure semantics.
3. Add CPU mock submit implementation and host tests.
4. Update track/docs only if the implementation discovers a contract gap.
