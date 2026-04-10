# NexusGfx Resource Model

**Created**: 2026-04-10  
**Owner**: @ui @runtime  
**Status**: Active architecture guidance for future `NexusGfx` tasks/RFCs

---

## Purpose

This document defines the **resource vocabulary** for `NexusGfx` so future tasks
can describe buffers, images, transient attachments, and interop consistently.

The resource model is the foundation for:

- games,
- CAD viewports,
- video/image editing,
- compute workloads,
- and future `NexusInfer` interop.

---

## Core resource classes

### Buffer

Byte-addressable storage used for:

- vertex/index data,
- uniform/constant-like data,
- storage buffers,
- staging uploads/downloads,
- indirect command data later,
- scientific/compute arrays,
- infer tensor payloads when the backend chooses a buffer form.

### Image

Structured pixel/plane-based storage used for:

- textures,
- render targets,
- transient attachments,
- depth/stencil-like attachments,
- video/image planes,
- sampled image inputs,
- storage-image style compute targets.

### Sampler

Sampling state separate from image ownership.

### Pipeline artifact reference

An immutable identifier that binds executable shader/kernel intent to a resource
layout and execution configuration.

---

## Backing posture

`NexusGfx` should align with the OS bulk-buffer model:

- **VMO** for OS production paths,
- **filebuffer** for host/testing/export analogs.

Rule:

- app/runtime/control metadata belongs on the control plane,
- large resource bytes belong on the data plane,
- resources are referenced by handles/slices, not embedded inline.

---

## Residency classes

Every resource should conceptually fall into one of these classes:

- `host_visible`
- `device_private`
- `transient`
- `streamed`
- `imported`
- `exported`

### `host_visible`

- CPU-readable or CPU-writable by contract
- appropriate for staging, readback, or CPU reference backends

### `device_private`

- optimized for device execution
- no assumption of direct host mapping

### `transient`

- valid for a bounded pass/window
- intended to avoid unnecessary external memory traffic
- ideal for tile-aware or temporary attachments

### `streamed`

- resource is not expected to remain fully resident
- suitable for large geometry, large textures, or artifact chunks

### `imported` / `exported`

- cross-subsystem interop:
  - window surfaces
  - media frames
  - infer tensors
  - capture/export flows

---

## Subresource vocabulary

The runtime should support explicit subresource references rather than forcing
whole-resource ownership everywhere.

Suggested concepts:

- byte slice
- image plane
- array layer
- mip-like level
- tile/chunk region
- attachment view

Why:

- CAD and large-scene workloads need partial streaming,
- video/image workflows need plane-aware interop,
- infer/gfx interop may want image views or tensor-like slices without copying.

---

## Access classes

Resources should declare intended access:

- `read_only`
- `write_only`
- `read_write`
- `sampled`
- `storage`
- `presentable`

These are not necessarily the final API names, but the distinction must exist.

Rule:

- write intent must be explicit,
- presentable resources are a special subset,
- and imported resources must carry conservative access defaults.

---

## Lifetime posture

Resource lifetime should be explicit and bounded:

- create/import,
- use across passes/submissions,
- signal completion,
- recycle/destroy.

Do not rely on:

- hidden refcount-driven survival in hot paths,
- accidental cache retention,
- or implicit "the driver probably keeps it alive".

Fences and submission completion should be the main lifetime transition points
for device-owned resources.

---

## Staging posture

Use explicit staging instead of magical upload/download behavior.

### Uploads

- `host_visible` staging buffer/image
- explicit copy/import into execution resource

### Downloads

- explicit copy/readback target
- bounded readback size
- deterministic completion behavior

Why:

- keeps resource ownership understandable,
- aligns CPU reference and real-device behavior,
- and helps later profiling.

---

## Transient attachments

Transient attachments are first-class because mobile/tile-aware GPUs are a
likely target.

Use them for:

- intermediate color/depth surfaces,
- short-lived G-buffer-like attachments,
- MSAA intermediates,
- temporary compute/render working sets.

Rule:

- if a pass result does not need to survive beyond a bounded phase, prefer a
  transient resource class in the design.

---

## Import/export posture

`NexusGfx` must support interop without inventing ad-hoc copies.

Important interop classes:

- `windowd` / present surfaces
- media frames
- image editing surfaces
- infer tensors and image-tensor views
- capture/export flows

Required posture:

- import/export is explicit,
- ownership and access rights are explicit,
- synchronization with fences is explicit,
- no implicit aliasing between unrelated clients.

---

## Resource budgets

The future contract should include bounded budgets for:

- total live buffers/images,
- per-queue or per-client transient bytes,
- staging pool bytes,
- imported resource count,
- cached pipeline/resource views,
- and readback bandwidth/size limits where relevant.

This is required for:

- deterministic behavior,
- crash containment,
- and multi-client fairness.

---

## Workload notes

### Games

- need textures, geometry buffers, transient render targets, presentable images
- strongly benefit from explicit transient resource classes

### CAD

- need large streaming buffers/images
- need subresource and selection-buffer posture

### Video editing

- needs plane-aware image resources and explicit import/export
- likely needs timeline-synchronous resource ownership

### Scientific compute

- needs large storage buffers
- less attachment-centric, more slice/buffer-centric

### Inference interop

- image/tensor interchange should reuse resource references, not reinvent a
  separate payload model

---

## First milestone guidance

The first milestone should only require a minimal set:

- buffers
- images
- transient attachments
- staging posture
- import/export hooks
- basic resource access/state validation

No sparse resources or exotic residency features are required in the first slice.

---

## Related

- `docs/architecture/nexusgfx-sync-and-lifetime.md`
- `docs/architecture/nexusgfx-command-and-pass-model.md`
- `docs/architecture/nexusgfx-tile-aware-design.md`
- `tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md`
