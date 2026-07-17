# NexusGfx Capability Matrix

**Created**: 2026-04-10  
**Owner**: @ui @runtime  
**Status**: Active architecture guidance for backend-independent planning

---

## Purpose

This document defines the **capability vocabulary** `NexusGfx` and related
systems should use to reason about backends.

The point is to avoid architecture drift such as:

- "this needs CUDA"
- "this only works on desktop GPUs"
- "this probably works on the future Imagination backend"

Instead, later tasks/RFCs should state which capabilities are required or
optional.

---

## Core rule

Backends should be described by **capabilities**, not by brand names.

Good:

- `supports_transient_attachments`
- `supports_storage_images`
- `supports_async_copy`

Bad:

- "Nvidia-like"
- "Metal-only"
- "CUDA-fast path"

---

## Capability groups

### Resource capabilities

- `supports_host_visible_buffers`
- `supports_device_private_buffers`
- `supports_transient_attachments`
- `supports_storage_images`
- `supports_subresource_views`
- `supports_imported_resources`
- `supports_exported_resources`

### Synchronization capabilities

- `supports_timeline_fences`
- `supports_waitsets`
- `supports_deadline_waits`
- `supports_cross_queue_sync`

### Pass and rendering capabilities

- `supports_render_passes`
- `supports_compute_passes`
- `supports_copy_blit_passes`
- `supports_tile_local_optimizations`
- `supports_resolve_attachments`

### Compute capabilities

- `supports_storage_buffer_compute`
- `supports_image_compute`
- `supports_shared_local_memory`
- `supports_indirect_dispatch`
- `supports_async_copy`

### Artifact/toolchain capabilities

- `supports_offline_pipeline_artifacts`
- `supports_runtime_pipeline_warmup`
- `supports_specialization_variants`

### Interop capabilities

- `supports_presentable_images`
- `supports_media_frame_import`
- `supports_image_tensor_interop`
- `supports_infer_gfx_interop`

---

## Suggested backend profiles

These are planning examples, not normative truth.

### `cpu_ref`

Likely:

- strong determinism
- host-visible resources
- render/compute/copy pass support in software
- limited transient realism
- no special tile-local hardware behavior

### `mobile_tiled_gpu`

Likely:

- transient attachment support
- tile-local optimization opportunities
- strong bandwidth sensitivity
- explicit pass structure strongly matters

### `npu_interop`

Likely:

- no general graphics pipeline
- strong tensor/compute specialization
- selective infer/gfx interop capability

---

## Why the matrix matters

This matrix should drive:

- profile lowering
- validation rules
- artifact specialization
- fallback planning
- task scoping

Examples:

- a first milestone can require only resource + sync + basic pass capabilities
- a later compute task can require `supports_storage_buffer_compute`
- a tile-aware optimization task can require `supports_transient_attachments`

---

## First milestone guidance

The first `NexusGfx` milestone should likely assume only a small subset:

- host-visible resources
- explicit passes
- bounded submit/sync
- present integration hooks
- offline artifacts

Everything else should remain optional or future-capability-gated.

---

## Related

- `docs/architecture/nexusgfx-compute-and-executor-model.md`
- `docs/architecture/nexusgfx-resource-model.md`
- `docs/architecture/nexusgfx-tile-aware-design.md`
