# NexusGfx Compute Kernel Model

**Created**: 2026-04-10  
**Owner**: @ui @runtime  
**Status**: Active architecture guidance for future compute tasks

---

## Purpose

This document captures how `NexusGfx` should think about **general compute**
without inheriting CUDA as the architectural default.

The goal is to support:

- postprocess and image kernels,
- game/simulation helpers,
- CAD assists,
- video/image compute passes,
- and scientific/array-oriented workloads,

through a portable kernel/dispatch model that remains compatible with CPU-first
proofs and future hardware backends.

---

## Scope boundary

This document is about **portable compute under `NexusGfx`**.

It is not:

- the `NexusInfer` session/graph contract,
- a vendor-specific GPU programming model,
- or a promise that v1 includes every HPC primitive.

`NexusInfer` may later reuse this substrate through interop, but it remains a
distinct runtime.

---

## Core model

Portable compute in `NexusGfx` should be framed around:

- **kernel artifact**
- **dispatch description**
- **resource bindings**
- **bounded workgroup shape**
- **explicit synchronization**

This is enough to support a useful v1 without locking the architecture to a
specific backend language.

---

## Kernel abstraction

A compute kernel should conceptually define:

- inputs/outputs
- binding layout
- workgroup requirements
- specialization knobs
- capability requirements

The architecture should not require the public API to expose:

- CUDA-specific launch syntax
- vendor-specific shared-memory semantics
- or backend-specific wave/warp terms

Those may exist in lowerings, not in the portable contract.

---

## Dispatch abstraction

Dispatch should be explicit and bounded.

The runtime needs vocabulary for:

- global work shape
- local/workgroup shape
- bounded specialization values
- dispatch ordering
- completion fence

This is sufficient for:

- image filters
- reduction helpers later
- simulation kernels
- scientific array kernels
- and gfx/infer interop experiments

---

## Resource binding posture

Compute should operate on the same resource model as the rest of `NexusGfx`:

- buffers
- images
- subresource views
- imported/exported resources

This avoids creating a separate compute-only memory universe.

Rule:

- compute resources use the same handle/slice/budget semantics as render
  resources,
- synchronization remains explicit and shared with the rest of the stack.

---

## Local/shared scratch memory

Some backends will support fast local/shared scratch memory; others will not.

Therefore:

- local scratch should be a **capability**, not a guaranteed baseline,
- portable kernels should either:
  - have a fallback path,
  - or declare the capability requirement explicitly.

This is another reason not to make CUDA-style assumptions part of the contract.

---

## Numeric posture

Compute kernels need explicit expectations for numeric behavior.

The architecture should distinguish:

- exact-by-contract operations
- tolerance-bounded floating-point operations
- backend-native fast paths
- quantized or mixed-precision variants

This matters for:

- scientific compute,
- image processing,
- geometry helpers,
- and later infer interop.

---

## Workload categories

### Graphics-adjacent compute

- image filters
- postprocess
- temporal reprojection helpers
- culling/visibility helpers later

### Product compute

- video compositing helpers
- editor effects
- CAD assists
- simulation for games/tools

### Scientific compute

- bounded array kernels
- reductions/scans later
- matrix/linear algebra helpers later

The architecture should be broad enough for these categories even if v1 only
implements a small subset.

---

## CPU reference importance

The compute model must remain meaningful on the CPU backend.

That means:

- kernels or equivalent artifacts can execute in a CPU reference environment,
- dispatch semantics are testable without real GPU hardware,
- correctness and boundedness can be proven host-first.

This is critical for the first milestone.

---

## Relationship to future IR

The compute model strongly benefits from a future mid-level IR, but the first
milestone does not require a massive compiler project.

For v0/v1, it is enough that the architecture already reserves room for:

- capability-driven lowering
- specialization
- deterministic artifact IDs
- multiple backend targets later

---

## First milestone guidance

The first compute slice under `NexusGfx` should stay small:

- explicit dispatch
- storage-buffer and image-adjacent compute posture
- shared resources/fences with graphics
- CPU reference execution
- deterministic host proofs

No giant HPC feature matrix is required for the first milestone.

---

## Not in first milestone

- no CUDA-specific kernel model
- no requirement for advanced tensor-core-like hardware features
- no unbounded autotuning
- no fully general scientific-computing library stack
- no assumption that GPU compute is available on day one

---

## Related

- `docs/architecture/nexusgfx-compute-and-executor-model.md`
- `docs/architecture/nexusgfx-resource-model.md`
- `docs/architecture/nexusgfx-artifact-pipeline.md`
- `tasks/TRACK-NEXUSGFX-SDK.md`
