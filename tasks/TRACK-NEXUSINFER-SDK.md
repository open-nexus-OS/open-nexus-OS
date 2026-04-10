---
title: TRACK NexusInfer SDK (on-device ML): runtime + CPU reference path + future NPU — zero-copy, policy-gated, QEMU-testable
status: Living
owner: @runtime @ui
created: 2026-04-10
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Extracted (NexusInfer v1b interop/profile binding): tasks/TASK-0280B-nexusinfer-v1b-tensor-image-interop-profile-binding.md
  - Extracted (NexusInfer v1c ownership + CPU reference executor): tasks/TASK-0284B-nexusinfer-v1c-ownership-based-tensor-buffer-states-cpu-reference-executor.md
  - Techniques catalog: docs/architecture/nexusinfer-techniques.md
  - Runtime profile vocabulary: docs/architecture/nexusinfer-runtime-profiles.md
  - Rust ownership/type design: docs/architecture/nexusinfer-rust-design.md
  - Gfx compute/executor model: docs/architecture/nexusgfx-compute-and-executor-model.md
  - Gfx resource model: docs/architecture/nexusgfx-resource-model.md
  - Gfx sync/lifetime model: docs/architecture/nexusgfx-sync-and-lifetime.md
  - Gfx capability matrix: docs/architecture/nexusgfx-capability-matrix.md
  - IPC hybrid (control Cap'n Proto + data VMO): docs/rfcs/RFC-0005-kernel-ipc-capability-model.md
  - Service architecture (data plane): docs/adr/0017-service-architecture.md
  - Drivers & accelerators (NPU device-class; submit/fence/budgets): tasks/TRACK-DRIVERS-ACCELERATORS.md
  - DriverKit core v1: tasks/TASK-0280-driverkit-v1-core-contracts-queues-fences-buffers.md
  - DMA buffer ownership prototype: tasks/TASK-0284-userspace-dmabuffer-ownership-v1-prototype.md
  - Zero-copy VMOs: tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - QoS/timers: tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md
  - Device MMIO access: tasks/TASK-0010-device-mmio-access-model.md
  - Deterministic parallelism: tasks/TASK-0276-parallelism-v1-deterministic-threadpools-policy-contract.md
  - Policy / capability matrix direction: tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - NexusGfx SDK (compute/present consumers): tasks/TRACK-NEXUSGFX-SDK.md
  - NexusMedia SDK (audio/video/image ML consumers): tasks/TRACK-NEXUSMEDIA-SDK.md
  - NexusGame SDK (realtime ML-assisted features): tasks/TRACK-NEXUSGAME-SDK.md
  - Search v2 lexical baseline (no ML in v2.1): tasks/TASK-0153-search-v2-backend-host-index-ranking-analyzers-sources.md
  - Search v2.1 semantic-lite (deterministic, no embeddings): tasks/TASK-0213-search-v2_1a-host-semvec-tags-expansion-hybrid.md
---

## Goal (track-level)

Deliver a first-party **NexusInfer** layer: a **single OS-wide on-device inference stack** (sessions, graphs, tensor I/O, budgets, power hints) that:

- **reuses the same hybrid IPC pattern as the rest of the system**: small **Cap’n Proto** control messages + **VMO/filebuffer** bulk data plane (no oversized inline tensor payloads),
- ships a **CPU reference backend first** so **host tests and QEMU** can prove end-to-end behavior **without NPU hardware**,
- adds **NPU (and later other accelerators)** as **alternate executors** behind the **same** session/API surface (`TRACK-DRIVERS-ACCELERATORS`, candidate **CAND-DRV-040**),
- stays **capability-first**, **bounded**, and **audit-friendly** (`policyd` authority; no ambient “ML device”),
- aligns **consumers** (media, games, system UI, future search) so they do **not** invent parallel inference stacks.

## Non-Goals

- This file is **not** an implementation task by itself.
- **No kernel ML** and no new syscall assumptions beyond existing IPC/VMO/cap models.
- **No cloud requirement**; optional remote orchestration belongs under **NexusNet** / product tasks, not the core runtime contract.
- **No bit-exact cross-backend guarantees** by default: CPU vs NPU numerics may differ; tests must document **tolerance vs determinism** per op class.
- **No vendor-locked contract surface**: NexusInfer must not require CUDA, Tensor Cores, or other hardware-specific APIs in its portable design.
- No QEMU markers are required for **this track document**; spawned tasks define markers.

## Control plane vs data plane (required)

Normative pattern (same as RFC-0005 and service onboarding):

| Plane | Mechanism | Contents |
|-------|-----------|----------|
| **Control** | Cap’n Proto (IDL) | session/graph handles, tensor **metadata** (shape, dtype, layout, offsets), fence/QoS IDs, error labels, bounded sizes |
| **Data** | **VMO** (OS) / **filebuffer** (host/QEMU analog) | weights, activations, images, large I/O — referenced by **handle id + slice**, not embedded in Cap’n Proto |

Large tensors **MUST NOT** be serialized inline in IPC once VMO/filebuffer paths exist for that consumer.

## CPU reference path (Phase 0 gate)

- A **CPU backend** is the **default executor** for bring-up: deterministic **fixtures**, bounded work, explicit **deadlines**, no hidden busy loops.
- **QEMU/host** proofs use the same API as future NPU: only the **executor** changes.
- Markers and tests for extracted tasks must distinguish **“pipeline works”** from **“NPU fast path”** — no fake `ready` for hardware that is not present.

## Accelerator path (Phase 1+)

- **NPU** maps to **CAND-DRV-040** in `TRACK-DRIVERS-ACCELERATORS` until extracted as a real `TASK-XXXX`.
- Executor selection is **policy- and capability-gated**; fallback to CPU remains **first-class** (degraded but safe).
- **GPU compute** for ML is a **future optional** alignment with **NexusGfx** / driver contracts — must not fork buffer/fence semantics.
- Hardware assumptions remain **generic**: future Imagination-backed GPU compute or custom NexusGfx interop must map into hardware-neutral capabilities, not leak vendor-specific primitives into app/runtime contracts.

## Consumers (intentional; avoid drift)

| Consumer | Relationship |
|----------|----------------|
| **NexusMedia** | analysis, enhancement, classification on frames/audio — uses shared tensor + fence model |
| **NexusGame** | optional realtime segmentation, upscaling, tracking — same runtime; see `TRACK-NEXUSGAME-SDK` candidates |
| **NexusGfx** | optional compute/present interop — shared VMO/fence vocabulary |
| **Search** | **TASK-0213 / v2.1** remains **lexical + semantic-lite without ML**. A **future** semantic/embedding layer **may** call NexusInfer in a **separate task**; do not redefine v2.1 scope. |

## UI / DSL integration posture

`NexusInfer` is **not** a layout, text, or retained-tree authority.

Inference results should flow back into the system through:

- state updates,
- effect results,
- model/content values,
- or explicitly bounded native-surface inputs.

The canonical UI contracts remain:

- `docs/dev/ui/foundations/layout/layout-pipeline.md`
- `docs/dev/ui/foundations/layout/text.md`
- `docs/dev/dsl/ir.md`

That means `NexusInfer` must **not** redefine or bypass:

- text preparation,
- measure/placement semantics,
- retained identity,
- paint-only vs layout-affecting field classification,
- or viewport/virtualization ownership.

If inference produces OCR, summaries, translations, semantic labels, or ranked results, those outputs should re-enter
the existing DSL/state/layout pipeline instead of creating a parallel UI authority.

## Image / tensor interop posture

`NexusInfer` should use the same low-level interop posture as `NexusGfx` and other accelerator consumers:

- resources travel as VMO/filebuffer-backed payloads with explicit handles/slices,
- ownership transfer is synchronized with fences/deadlines,
- image/frame/tensor import and export stay bounded and explicit,
- and backend choice remains capability-driven.

Do **not** create a separate infer-only image/buffer universe when the same resource model, sync model, and capability
language can be shared with `NexusGfx`.

For image/video/tensor style workflows, `NexusInfer` should align with:

- `docs/architecture/nexusgfx-compute-and-executor-model.md`
- `docs/architecture/nexusgfx-resource-model.md`
- `docs/architecture/nexusgfx-sync-and-lifetime.md`
- `docs/architecture/nexusgfx-capability-matrix.md`

This keeps graphics, media, and inference interop zero-copy-friendly and prevents later drift into parallel buffer
contracts.

## Gates (RED / YELLOW / GREEN)

- **RED**: no real cross-process inference contract without **TASK-0031**-backed bulk handles and bounded validation.
- **YELLOW**: numerical tolerance vs deterministic goldens; vendor plugin boundaries for NPU dispatch; supply chain for model artifacts.
- **GREEN**: hybrid Cap’n Proto + VMO/filebuffer is the **only** approved bulk pattern; CPU reference path is **testable** before any NPU.

## Candidate subtasks (extract to `TASK-XXXX` when gates clear)

- **CAND-INFER-000: Inference session API v0 + CPU backend + host proofs**  
  - minimal session lifecycle, tensor VMO attach, bounded graph/submit, deterministic fixture  
  - proof: host tests; optional QEMU markers for “fixture inference completed” (no NPU claims)
  - **Status**: extracted in parts → `TASK-0280B` (interop/profile binding) and `TASK-0284B` (ownership + CPU reference executor)

- **CAND-INFER-010: IDL schemas (`*.capnp`) for infer control plane**  
  - metadata-only in wire messages; tensor bytes in VMO; explicit caps for model load
  - **Status**: still candidate

- **CAND-INFER-020: Policy + audit hooks for `infer.*` capabilities**  
  - align with `TASK-0136` / policy engine; deny-by-default model load and sensor-adjacent ops
  - **Status**: still candidate

- **CAND-INFER-030: NPU executor integration**  
  - depends on extracted NPU device-class task (from **CAND-DRV-040**); same session API
  - **Status**: still candidate

## First extraction preference

When this track first becomes a real `TASK-XXXX`, prefer the **smallest CPU-reference slice** that locks the
runtime contract without requiring accelerator hardware:

- session lifecycle + profile selection,
- tensor metadata on the **Cap'n Proto control plane**,
- tensor/model payloads on the **VMO/filebuffer data plane**,
- bounded submit/completion semantics,
- deterministic host proofs and optional QEMU fixture markers.

The first extracted task should **not** depend on real NPU/GPU execution. The goal of the first slice is to prove the
**portable runtime shape** so later CPU/NPU/Gfx executors plug into the same contract.

## Not in v1

The first NexusInfer task/RFC should explicitly avoid the following scope creep:

- **no mandatory NPU path** in the first extracted task,
- **no vendor-specific compute API** assumptions (CUDA, Tensor Cores, warp-level primitives, etc.),
- **no Search v2.1 ML rewrite**; `TASK-0213` remains semantic-lite and non-embedding-based,
- **no requirement to implement TurboQuant or any specific KV-compression algorithm in v1**,
- **no multimodal-everything requirement**; text-first or tightly bounded fixture inputs are acceptable,
- **no hidden automatic profile switching** without an explicit runtime/profile vocabulary.

## Extraction rules

Extract a candidate only when it has:

- deterministic or explicitly tolerance-bounded proofs (host-first; QEMU where meaningful),
- explicit **minimal v1** vs **deluxe** boundaries,
- no ad-hoc parallel buffer or IPC schemes (stay on RFC-0005 hybrid),
- security section for untrusted models and oversized inputs (`test_reject_*` pattern per project standards).

## See also

- Networking-style bulk patterns: `tasks/TRACK-NEXUSNET-SDK.md`
- Zero-copy app platform: `tasks/TRACK-ZEROCOPY-APP-PLATFORM.md`
- Keystone dependency: `tasks/TRACK-KEYSTONE-GATES.md` (VMO proofs)
