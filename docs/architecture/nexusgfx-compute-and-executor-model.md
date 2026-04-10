# NexusGfx Compute and Executor Model

**Created**: 2026-04-10  
**Owner**: @ui @runtime  
**Status**: Active architecture guidance for `tasks/TRACK-NEXUSGFX-SDK.md`

---

## Purpose

This document defines the **layering model** for graphics and compute in Open
Nexus OS so later tasks/RFCs can derive a stable v0/v1 plan without guessing.

It answers:

- how `NexusGfx` and `NexusInfer` relate,
- what belongs in shared primitives vs backend code,
- how executor classes are chosen,
- and why the architecture is **not CUDA-first**.

This page is architecture guidance, not the final API or wire contract.

---

## Executive stance

Open Nexus OS should treat graphics and general compute as a **single explicit,
portable acceleration stack** with:

- **shared primitives** for buffers, fences, queues, deadlines, and budgets,
- **separate product-facing SDKs** (`NexusGfx`, `NexusInfer`, later media-facing
  consumers),
- **thin hardware backends** for CPU, future GPU, and future NPU paths,
- and **policy-/capability-gated executor selection**.

The goal is not to replicate Vulkan, CUDA, or Metal literally. The goal is to
provide the **same category of power** while keeping:

- kernel complexity low,
- portability high,
- and hardware assumptions generic.

---

## Layer model

### 1. App-facing SDK layer

This is what apps and first-party product code use.

- `NexusGfx` exposes graphics, compute, and present concepts for apps/games.
- `NexusInfer` exposes inference sessions, tensors, profiles, and executors.
- Product tracks such as media, games, CAD, and video editing build on these
  SDKs instead of talking to device services directly.

Rule:

- apps do **not** get direct MMIO/IRQ/DMA authority,
- apps do **not** target vendor APIs,
- apps do **not** own the policy model.

### 2. Planner and validation layer

This layer remains portable and owns the **high-level execution model**:

- resource state validation,
- queue/submit planning,
- pass planning,
- budget enforcement,
- residency and staging decisions,
- profile selection and fallback.

This is where the OS decides "what should happen", not how a specific GPU or
NPU encodes commands.

### 3. IR and artifact layer

This layer turns high-level SDK intent into stable, cacheable artifacts:

- shader/pipeline artifacts for `NexusGfx`,
- dispatch descriptions and kernel artifacts for compute,
- graph/session/profile artifacts for `NexusInfer`,
- stable IDs and versioning for caches and signatures.

This layer exists to keep compilation deterministic and backend-agnostic.

### 4. Executor and backend layer

This layer is backend-specific but should remain thin.

Examples:

- `cpu_ref`
- `cpu_optimized`
- `gfx_compute`
- `npu`

Backends implement the contracts defined above:

- resource import/export,
- submit,
- fence signaling,
- reset/recovery hooks,
- capability reporting.

### 5. DriverKit and device-service layer

This layer owns:

- MMIO programming,
- firmware protocols,
- command encoding,
- reset/recovery,
- device bring-up,
- audited fault handling.

This is the only place where real hardware details should dominate.

---

## `NexusGfx` vs `NexusInfer`

### `NexusGfx`

`NexusGfx` is the explicit graphics/compute SDK for:

- UI acceleration,
- games,
- CAD viewports,
- video/image processing primitives,
- and later portable compute kernels.

Its core posture:

- resource-explicit,
- pass-explicit,
- sync-explicit,
- artifact-driven,
- and suitable for CPU-first proofs.

### `NexusInfer`

`NexusInfer` is the system inference runtime for:

- local assistant features,
- image/audio/video analysis,
- AI-assisted editing,
- and future search/ranking layers where explicitly scoped.

Its core posture:

- session/graph/tensor-oriented,
- profile-driven,
- bounded and policy-gated,
- CPU reference path first,
- future NPU or GPU compute executor optional.

### Relationship

The two systems must **share low-level acceleration primitives** but remain
product-distinct:

- `NexusGfx` should not become "the ML API".
- `NexusInfer` should not redefine graphics resources or synchronization.

Shared primitives should live conceptually in a common acceleration substrate:

- buffers/images/tensors as bulk payload references,
- fences/waitsets/deadlines,
- queue submission semantics,
- import/export and interop rules,
- resource budgets and residency classes.

---

## Shared primitives

The following concepts should stay aligned across graphics, compute, and infer:

- **resource references**: VMO/filebuffer-backed buffers and images
- **subresources/slices**: offsets, ranges, layers, planes, mip-like subsets
- **queues**: explicit submit and bounded in-flight work
- **fences**: timeline-oriented completion and wait semantics
- **deadlines**: QoS-aware timing hints and bounded wait behavior
- **budgets**: bytes, queue depth, residency caps, cache limits
- **fault/reset**: explicit, audited reset/recovery surfaces
- **capabilities**: backend reports features; upper layers decide policy

---

## Executor classes

The runtime should treat executor choice as an explicit policy/profile decision.

### `cpu_ref`

- deterministic proof path
- minimal baseline behavior
- host-first and QEMU-friendly
- no hardware dependency

### `cpu_optimized`

- CPU path still using the same contracts
- used when no accelerator is available or when thermal/power policy prefers CPU

### `gfx_compute`

- optional compute path exposed through `NexusGfx`
- useful for:
  - postprocess
  - image filters
  - video graph helpers
  - some simulation/CAD/scientific kernels
  - future `NexusInfer` interop

### `npu`

- future executor for inference-oriented workloads
- session/profile surface remains the same as CPU reference

Rule:

- executor classes must be visible to policy and debugging,
- not silently swapped by hidden heuristics.

---

## Capability-driven lowering

Lowering should proceed by **required capabilities**, not by vendor name.

Examples of portable capability concepts:

- `supports_transient_attachments`
- `supports_storage_images`
- `supports_async_copy`
- `supports_timeline_fences`
- `supports_quantized_kv`
- `supports_fast_storage_parameter_streaming`
- `supports_shared_local_memory`
- `supports_image_tensor_interop`

Why:

- future Imagination-backed GPUs do not look like CUDA,
- CPU backends must still participate,
- NPU paths may support some compute categories but not general graphics,
- and capability checks are more stable than backend brand checks.

---

## Why not CUDA-first

CUDA is useful as a **reference point**, not as the contract model.

Reasons:

1. **Hardware mismatch**
   - Open Nexus OS is more likely to target mobile/tile-aware GPUs and future NPU
     services than Nvidia-centric compute stacks.
2. **Portability**
   - CUDA terms do not map cleanly to CPU reference, Imagination-style GPUs, or
     generic userspace driver services.
3. **TCB and policy**
   - CUDA-first design tends to drag vendor-specific runtime assumptions into the
     core architecture.
4. **API drift**
   - We want portable concepts such as passes, queues, fences, budgets, and
     capability matrices, not vendor-specific kernel/stream semantics.

The right posture is:

- learn from CUDA's execution ideas,
- but document generic equivalents in the architecture.

---

## Workload classes this model must support

The design should remain credible for all of these without forking the contract:

- **Games**: frame-paced rendering, compute effects, resource streaming
- **CAD**: large meshes, picking, overlays, optional compute assists
- **Video editing**: timeline surfaces, image/video compute hooks, low-jitter present
- **Scientific compute**: bounded dispatch-based kernels, large storage buffers,
  reductions/scans later
- **Inference**: tensor I/O and profile-driven executor choice

The first milestone does **not** need to deliver all of these equally. But the
architecture should make them fit the same substrate.

---

## First milestone shape

The first `NexusGfx` extraction should prefer:

- CPU reference path first,
- explicit resources/queues/fences,
- bounded validation,
- 2D and basic 3D/compute substrate,
- deterministic host proofs,
- no real GPU dependency,
- no vendor-specific kernel language requirement.

This matches the project's host-first proof discipline and keeps the first task
small enough to be real.

---

## Not in v1

The first milestone should explicitly avoid:

- Vulkan/OpenGL compatibility as a design driver,
- CUDA-specific compute semantics,
- mandatory runtime shader JIT,
- mandatory real GPU in the first extraction,
- unbounded pipeline caches,
- and implicit synchronization magic.

---

## Related

- `tasks/TRACK-NEXUSGFX-SDK.md`
- `tasks/TRACK-NEXUSINFER-SDK.md`
- `docs/architecture/nexusgfx-resource-model.md`
- `docs/architecture/nexusgfx-sync-and-lifetime.md`
- `docs/architecture/nexusgfx-command-and-pass-model.md`
- `docs/architecture/nexusgfx-capability-matrix.md`
