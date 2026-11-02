---
schema: 1
id: TRACK-NEXUSGFX-SDK
title: "NexusGfx SDK (Metal-like, apps + games): contracts + phased roadmap (2D/3D/compute/audio/video/cad)"
status: living
owner: ["@ui", "@runtime"]
created: 2025-12-26
links:
  - label: Vision
    target: docs/architecture/vision.md
  - label: Playbook
    target: CLAUDE.md
  - label: "Extracted (NexusGfx v1b resource/fence core)"
    target: tasks/TASK-0169B-gpu-draws-gfxdevice-on-the-device-first-quad-offline-shaders.md
  - label: "Extracted (NexusGfx v1c windowd handoff)"
    target: tasks/TASK-0170B-nexusgfx-v1c-windowd-handoff-minimal-pass-planning.md
  - label: "Current state (2026-05-29)"
    target: "Module structure established (Phase 5 of RFC-0059). 10-directory Metal-like tree, 40+ skeleton files. Existing v0 code moved into core/, resource/, command/. Backward-compatible re-exports. See RFC-0059 for full structure."
  - label: "NexusMedia SDK track (audio/video/image)"
    target: tasks/TRACK-NEXUSMEDIA-SDK.md
  - label: "NexusGame SDK track (games)"
    target: tasks/TRACK-NEXUSGAME-SDK.md
  - label: "NexusNet SDK track (cloud + DSoftBus)"
    target: tasks/TRACK-NEXUSNET-SDK.md
  - label: "Drivers & accelerators foundations"
    target: tasks/TRACK-DRIVERS-ACCELERATORS.md
  - label: "NexusInfer SDK (optional ML compute interop; same VMO/fence vocabulary)"
    target: tasks/TRACK-NEXUSINFER-SDK.md
  - label: "NexusFrame (Pixelmator-class editor; reference workload)"
    target: tasks/TRACK-NEXUSFRAME.md
  - label: "Device/MMIO access model (gate)"
    target: tasks/TASK-0010-device-mmio-access-model.md
  - label: "Zero-copy VMOs (gate)"
    target: tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - label: "QoS/timers (soft real-time spine)"
    target: tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md
  - label: "Perf tracing + gates"
    target: tasks/TASK-0143-perf-v1a-perfd-frame-trace-metrics.md
  - label: "Perf hooks + HUD"
    target: tasks/TASK-0144-perf-v1b-instrumentation-hud-nx-perf.md
  - label: "Perf regression gates"
    target: tasks/TASK-0145-perf-v1c-deterministic-gates-scenes.md
---

> **Link 2026-09-21:** the GPU backend this SDK needs is target picture G in `tasks/IMPLEMENTATION-ORDER.md` (`TASK-0329` thin side, `TASK-0280` v2 API, 0169B/0170B/0171 driver, 0215/0216 whole desktop, `0329B` app surfaces); the SDK's app-facing phases start after G4/G5.


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
- **Hardware abstraction**: capability-driven lowering and executor selection; no CUDA/Tensor-Core assumptions in the portable contract.  
  Source: "Design notes" below (layer model, capability vocabulary).

## Architecture surfaces

Detailed design input for later task/RFC extraction lives in the "Design notes" section at the end of this track.

## Related tracks (intentional split, shared primitives)

- **Media/audio/video/images**: `tasks/TRACK-NEXUSMEDIA-SDK.md`
- **Games** (input/timing glue on top of NexusGfx + NexusMedia): `tasks/TRACK-NEXUSGAME-SDK.md`
- **On-device ML** (CPU reference + future NPU; shared buffers/fences): `tasks/TRACK-NEXUSINFER-SDK.md`

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

## Mobile / tile-aware stance

The likely real-device posture is **mobile/tile-aware**, not desktop-immediate-mode by default:

- preserve pass locality and transient attachments in the portable API/artifact model,
- optimize for **bandwidth-first** execution rather than brute-force memory traffic,
- and keep the architecture compatible with likely Imagination-style GPU behavior without hard-coding vendor APIs.

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
  - **Status**: extracted in parts → `TASK-0169B` (resource/fence core) and follow-up `TASK-0170B` for OS handoff alignment

- **CAND-GFX-001: Validation layer v0 (bounded, deterministic)**  
  - validate command buffers and resource states before submit  
  - deterministic “why denied” diagnostics (bounded)
  - **Status**: partially extracted → `TASK-0169B`

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
  - **Status**: first extraction → `TASK-0170B`

### Pro workloads

- **CAND-GFX-040: CAD v1 primitives (instancing, large meshes, selection buffers)** (host-first proofs)
- **CAND-GFX-050: Video editing substrate (timelines, decode/compose hooks)** (ties to VPU track)
- **CAND-GFX-060: Audio DAW UI substrate (low jitter UI + audio sync points)** (ties to audio track)

## First extraction preference

When this track first becomes a real `TASK-XXXX`, prefer the **smallest CPU-reference slice** that locks:

- the resource model,
- queue/fence/sync semantics,
- command/pass structure,
- offline-first artifact posture,
- and a minimal 2D + basic compute-capable execution shape.

The first extracted task should **not** require a real GPU. It should prove the portable contract so later Imagination-
style GPU, future compute, and `NexusInfer` interop plug into the same design.

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

## Not in v1

The first `NexusGfx` task/RFC should explicitly avoid:

- Vulkan/OpenGL compatibility as a design driver,
- CUDA-specific compute semantics,
- mandatory runtime shader JIT,
- mandatory real GPU hardware in the first extraction,
- unbounded pipeline/artifact caches,
- and forcing the existing UI text/layout contracts to be rewritten under `NexusGfx`.

## Extraction rules (how candidates become real tasks)

- A candidate becomes a real `TASK-XXXX` only when:
  - it is implementable under current constraints (or explicitly creates prerequisite tasks),
  - it has proof (deterministic host tests and/or QEMU markers where valid),
  - it documents “minimal v1” vs “future deluxe” explicitly to avoid drift.
- When extracted:
  - this entry is replaced with `Status: extracted → TASK-XXXX` and a link,
  - all detailed requirements live in the real task, not here.

## Design notes (merged from docs/architecture/graphics/nexusgfx-*, 2026-11-02)

Planning vocabulary for the future NexusGfx RFC; names are candidates, not API.

**Already built — do not re-plan:** `userspace/nexus-gfx/` has the command buffer, render encoder and layer wire format with bounded validation (`userspace/nexus-gfx/src/command/`), the timeline-fence model (`userspace/nexus-gfx/src/sync/timeline.rs`, RFC-0033), the v2 device contract `GfxDevice` with its host oracle `cpu_mock` (`userspace/nexus-gfx/src/backend/v2/`, RFC-0101) and the capability report (`source/libs/nexus-driverkit/src/gpu.rs`), the offline pipeline library + IR + per-target code backends (`userspace/nexus-gfx/src/shader/`, ADR-0072, built by `tools/nxgpu`), the frame planner (`userspace/nexus-gfx/src/frame/`) and the one software rasterizer (`userspace/nexus-gfx/src/raster/`). The compute/blit encoders, passes, image/sampler/heap, barrier/event, `transfer/`, `cache/` and `pipeline/` modules are placeholders — the notes below are their design input.

### Layers and the shared substrate

| Layer | Owns |
|-------|------|
| App SDK (NexusGfx, NexusInfer) | product concepts; apps never get MMIO/IRQ/DMA, never target vendor APIs, never own policy |
| Planner + validation (portable) | resource-state validation, submit/pass planning, budgets, residency/staging, profile selection and fallback — *what* happens |
| IR + artifacts | pipeline, kernel and graph artifacts with stable IDs and versions |
| Executor backend (thin) | import/export, submit, fence signalling, reset hooks, capability report |
| DriverKit device service | MMIO, firmware protocol, command encoding, reset/recovery, audited faults — the only layer where hardware detail dominates |

- NexusGfx and NexusInfer share and never fork: VMO/filebuffer references and slices, queues with bounded in-flight work, timeline fences/waitsets/deadlines, budgets and residency classes, import/export rules, audited fault/reset. NexusGfx is not the ML API; NexusInfer does not redefine graphics resources or sync.
- Executor classes `cpu_ref` · `cpu_optimized` · `gfx_compute` · `npu` are visible to policy and debugging, never swapped by hidden heuristics. The `cpu_ref` floor is TASK-0054 / RFC-0046 (deterministic host frames + goldens); later backends keep its pixel, bounds and proof-honesty guarantees.

### Resources

- Classes: buffer (vertex/index, constants, storage, staging, indirect later, tensor payloads) · image (textures, render targets, depth-like, video planes, storage images) · sampler (state apart from image ownership) · pipeline-artifact reference (immutable; binds a program to a binding layout and execution config).
- Residency: `host_visible` · `device_private` · `transient` (bounded pass window — prefer whenever a result does not outlive it) · `streamed` (large geometry, textures, artifact chunks) · `imported` · `exported` (window surfaces, media frames, infer tensors, capture).
- Subresources: byte slice, image plane, array layer, mip-like level, tile/chunk region, attachment view — partial streaming and plane-aware interop without copies.
- Access: `read_only` · `write_only` · `read_write` · `sampled` · `storage` · `presentable`. Write intent explicit; imported resources default to conservative access.
- Staging explicit both ways: upload = host_visible staging + explicit copy/import; download = explicit, size-bounded readback target.
- Budgets: live buffers/images, transient bytes per queue/client, staging pool bytes, imported count, cached pipelines/views, readback size.
- No hidden refcount survival on hot paths; fences are the lifetime transition points.

### Sync and lifetime

- States: created → recorded-for-use → in-flight → completed → recycled → destroyed; imported: imported-visible → imported-in-flight → returned.
- Waits: non-blocking check, deadline-bounded wait, waitset over a bounded group. No wait-forever in the portable contract, no userspace spin.
- Every interop handoff (windowd, media frames, NexusInfer) states who owns the resource, which fence signals the handoff, which queue consumes next, and when the producer may reclaim.
- Present exposes frame-ready completion, pacing deadline, dropped-frame accounting, bounded frame-queue depth, presentable lifetime.
- Faults: stable error classes; in-flight resources recover only through an explicit, audited reset; per-client containment over global collapse; no silent retry of lost work.
- Validation rejects with a bounded why-denied: use-after-submit, double-submit, queue overflow, invalid fence wait, reuse before completion, imported/presentable misuse.

### Commands and passes

- Pass classes: render (raster, attachments, 2D/text/UI, depth/selection) · compute (dispatch, image/buffer processing, postprocess, simulation) · copy/blit (upload/download, resolve, copies) — kept apart for bandwidth attribution.
- A pass is one bounded execution region: not one per effect, not a kitchen sink that hides dependencies. Recording is deterministic: no wallclock input, no implicit pass insertion, no driver command repair.
- Dependencies = resource access intent + queue order + pass order + explicit fences across queue/owner boundaries; no vendor barrier formats in the portable API.
- Render-pass attachments carry explicit load / store / transient-only / resolve / consumed-next intent.
- Secondary bundles and indirect draws/dispatches: later, capability-gated, bounded.
- Every primitive validates and runs deterministically on the CPU backend; pass boundaries are real state transitions even when executed serially.

### Compute kernels

- A kernel declares inputs/outputs, binding layout, workgroup requirements, bounded specialization knobs and required capabilities. A dispatch carries global shape, workgroup shape, specialization values, ordering and a completion fence.
- Same resource/handle/budget/sync model as rendering — no compute-only memory universe.
- Local/shared scratch memory is a capability: a kernel declares it or has a fallback. Wave/warp terms live only inside lowerings.
- Numeric classes: exact-by-contract · tolerance-bounded float · backend-native fast path · quantized/mixed.
- Not first: autotuning, a general scientific library, any assumption that GPU compute exists.

### Tile-aware rules

- Transient candidates: G-buffer-like intermediates, depth/stencil scratch, MSAA intermediates, temporary lighting/composition targets, tile-local postprocess.
- Anti-patterns: redundant full-surface store/load; a full-frame resolve where a bounded one suffices; attachment oversubscription; barriers without a real dependency; every postprocess through an externally stored texture; compute as a universal replacement for pass fusion; splitting a producer from its immediate consumer.
- Pro surfaces: selection/highlight buffers without full-frame churn; timeline previews reuse transient or partial resources.
- The CPU backend cannot prove tile-memory behaviour; it proves that pass boundaries, load/store intent and transient declarations reach the backend.

### Text

- The UI stack owns resolve tree, text preparation, shaping/bidi, line layout, measure/placement, invalidation classes and identity (`docs/dev/ui/foundations/layout/text.md`, `docs/dev/ui/foundations/layout/layout-pipeline.md`). NexusGfx owns glyph materialization, atlas/cache lifetime, batching, damage-aware submission and the backend draw.
- The renderer consumes prepared paragraphs; scroll-only changes reuse prepared text and line layout; theme/colour changes are paint-only; a newly materialized glyph never forces an unrelated full redraw.
- Atlas: lazy fill, deterministic keys (font, glyph, size/style bucket, subpixel bucket if needed, colour/emoji mode), bounded count/size and eviction. Vector glyph rasterization only later, never per frame.
- Batch keys: atlas/resource, blend/colour mode, clip compatibility, transform, pipeline state; never batch across state that breaks determinism or damage accounting.
- Virtual lists: cache churn must not break anchor-by-key; visible-range changes do not invalidate the whole atlas.
- Colour glyphs are a materialization distinction, not a second layout pipeline; native surfaces still route text through the canonical contracts.

### Artifacts (beyond the pipeline library)

- Classes: source inputs, IR, pipeline artifacts, specialized variants, debug symbol/source-map sidecars, cache indexes/manifests — for graphics, compute, and shared infer graph artifacts.
- Identity inputs: canonical source/IR, stable config fields, required capabilities/profile, specialization values, artifact format version. Never: traversal order, temp paths, timestamps, map iteration order.
- Runtime cache: stable keys, bounded size, explicit warmup stages, no background harvesters, no runtime mutation that breaks reproducibility. Later: profile-driven or install-time prewarm.
- Specialization explicit, bounded and part of the identity; no near-duplicate variant explosion.
- Artifact IDs label traces and perf scenes; Dev Studio (`tasks/TRACK-DEVSTUDIO-IDE.md`) inspects, prebuilds, signs and deploys them.

### Capability vocabulary

Each entry is `supports_<name>`; tasks name required capabilities, never backend brands.

| Group | Capabilities |
|-------|--------------|
| Resource | host_visible_buffers, device_private_buffers, transient_attachments, storage_images, subresource_views, imported_resources, exported_resources |
| Sync | timeline_fences, waitsets, deadline_waits, cross_queue_sync |
| Pass | render_passes, compute_passes, copy_blit_passes, tile_local_optimizations, resolve_attachments |
| Compute | storage_buffer_compute, image_compute, shared_local_memory, indirect_dispatch, async_copy |
| Artifact | offline_pipeline_artifacts, runtime_pipeline_warmup, specialization_variants |
| Interop | presentable_images, media_frame_import, image_tensor_interop, infer_gfx_interop |

Backend profiles to plan against: `cpu_ref` (deterministic, host-visible, all pass classes in software, no tile behaviour) · `mobile_tiled_gpu` (transient + tile-local, bandwidth-sensitive, pass structure matters) · `npu_interop` (no graphics pipeline, tensor/compute-specialized, selective interop). The first milestone needs only host-visible resources, explicit passes, bounded submit/sync, present hooks and offline artifacts; everything else is capability-gated.
