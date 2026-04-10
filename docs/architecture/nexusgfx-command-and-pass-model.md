# NexusGfx Command and Pass Model

**Created**: 2026-04-10  
**Owner**: @ui @runtime  
**Status**: Active architecture guidance for future `NexusGfx` tasks/RFCs

---

## Purpose

This document defines how `NexusGfx` should think about command recording,
passes, and submission without inheriting vendor-specific API baggage.

The command/pass model exists to make:

- rendering explicit,
- compute explicit,
- resource transitions understandable,
- tile-aware planning possible,
- and CPU-first proofs realistic.

---

## Core command objects

The portable architecture expects concepts in this family:

- `Device`
- `Queue`
- `CommandBuffer`
- `Fence`
- `RenderPass`
- `ComputePass`
- `Copy/BlitPass`
- `Pipeline`

The exact final names may change, but these roles should exist.

---

## Recording posture

Command recording should be explicit and deterministic.

Recommended posture:

- command buffers are built from stable, bounded inputs,
- validation can inspect them before submit,
- recording does not depend on wallclock state,
- and command ordering is explicit in tests.

Do not assume:

- hidden deferred command mutation,
- implicit pass insertion,
- or best-effort driver command repair.

---

## Pass classes

### Render pass

Used for:

- rasterization,
- attachment-based rendering,
- text/2D/UI submission,
- depth/selection/present-preparation,
- tile-local operations where supported.

### Compute pass

Used for:

- general dispatch-based work,
- image/buffer processing,
- postprocess,
- simulation,
- future gfx-side compute interop with infer/media.

### Copy/Blit pass

Used for:

- uploads/downloads,
- resolves,
- copies between resources,
- format/layout-safe moves.

Keeping this separate makes profiling and bandwidth reasoning simpler.

---

## Pass-locality rule

A pass should represent a **bounded, meaningful execution region**.

Why this matters:

- on tile-aware/mobile GPUs it helps preserve locality,
- on CPU reference backends it keeps proof structure understandable,
- and in profiling it gives a usable unit of attribution.

Rule:

- do not explode one logical operation into unnecessary passes,
- but also do not create giant "kitchen sink" passes that hide dependencies.

---

## Attachment posture

Render passes may bind:

- color-like attachments,
- depth/stencil-like attachments,
- transient attachments,
- resolve targets,
- imported/presentable surfaces.

The architecture should make load/store intent explicit enough that later
backends can preserve tile locality or reuse transient storage correctly.

---

## Dispatch posture

Compute passes should expose:

- kernel/pipeline reference,
- workgroup/dispatch shape,
- buffer/image bindings,
- bounded constants/parameters,
- synchronization requirements.

The portable contract should not require CUDA thread-block terminology, but it
does need a generic dispatch vocabulary that can lower to CPU, future GPU, or
other compute executors.

---

## Command ordering and dependencies

Dependencies between passes should be represented by:

- resource access intent,
- queue ordering,
- pass order,
- explicit fences or waits when crossing queue/owner boundaries.

This should be sufficient for:

- validation,
- later backend lowering,
- performance reasoning,
- and deterministic replay.

---

## Secondary recording / bundles

These should be considered optional future features, not first-milestone
requirements.

Potential use cases:

- repeated scene sections
- repeated UI batches
- repeated postprocess chains

But do not force a "secondary command buffer" abstraction into v0 unless the
first milestone actually needs it.

---

## Indirect and generated work

Indirect draws/dispatches are valid future directions for:

- large scene rendering,
- culling,
- simulation,
- scientific compute,
- and pro graphics tooling.

However, they should be treated as:

- capability-gated,
- bounded,
- and later than the first milestone.

---

## CPU reference compatibility

The command/pass model must remain meaningful on the CPU backend.

That means:

- no command primitive should require a real GPU to be testable,
- command buffers should still validate and execute deterministically on CPU,
- and pass boundaries should still correspond to real state transitions even
  when the backend executes them serially.

This is essential for host-first proofs.

---

## Relationship to tile-aware design

The pass model must be compatible with:

- transient attachments,
- minimal store/load churn,
- merged/local passes where useful,
- and preserving data locality across pass boundaries.

Therefore pass planning is not merely an API concern; it is a performance model.

See:

- `docs/architecture/nexusgfx-tile-aware-design.md`

---

## First milestone guidance

The first milestone should likely include only:

- command buffer recording,
- render pass,
- compute pass,
- copy/blit pass,
- basic submit,
- and fence completion.

That is sufficient for:

- 2D,
- basic 3D,
- upload/download,
- text/path subsets,
- and future compute growth.

---

## Related

- `docs/architecture/nexusgfx-resource-model.md`
- `docs/architecture/nexusgfx-sync-and-lifetime.md`
- `docs/architecture/nexusgfx-tile-aware-design.md`
