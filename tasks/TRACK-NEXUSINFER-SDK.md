---
schema: 1
id: TRACK-NEXUSINFER-SDK
title: "NexusInfer SDK (on-device ML): runtime + CPU reference path + future NPU — zero-copy, policy-gated, QEMU-testable"
status: living
owner: ["@runtime", "@ui"]
created: 2026-04-10
links:
  - label: Vision
    target: docs/architecture/vision.md
  - label: Playbook
    target: CLAUDE.md
  - label: "Extracted (NexusInfer v1b interop/profile binding)"
    target: tasks/TASK-0280B-nexusinfer-v1b-tensor-image-interop-profile-binding.md
  - label: "Extracted (NexusInfer v1c ownership + CPU reference executor)"
    target: tasks/TASK-0284B-nexusinfer-v1c-ownership-based-tensor-buffer-states-cpu-reference-executor.md
  - label: "Gfx compute/executor model"
    target: tasks/TRACK-NEXUSGFX-SDK.md
  - label: "Gfx resource model"
    target: tasks/TRACK-NEXUSGFX-SDK.md
  - label: "Gfx sync/lifetime model"
    target: tasks/TRACK-NEXUSGFX-SDK.md
  - label: "Gfx capability matrix"
    target: tasks/TRACK-NEXUSGFX-SDK.md
  - label: "IPC hybrid (control Cap'n Proto + data VMO)"
    target: docs/rfcs/RFC-0005-kernel-ipc-capability-model.md
  - label: "Service architecture (data plane)"
    target: docs/adr/0017-service-architecture.md
  - label: "Drivers & accelerators (NPU device-class; submit/fence/budgets)"
    target: tasks/TRACK-DRIVERS-ACCELERATORS.md
  - label: "DriverKit core v1"
    target: tasks/TASK-0280-gpu-and-scanout-base-contracts-driverkit-v2-gfxdevice-v2-host.md
  - label: "DMA buffer ownership prototype"
    target: tasks/TASK-0284-userspace-dmabuffer-ownership-v1-prototype.md
  - label: "Zero-copy VMOs"
    target: tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - label: QoS/timers
    target: tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md
  - label: "Device MMIO access"
    target: tasks/TASK-0010-device-mmio-access-model.md
  - label: "Deterministic parallelism"
    target: tasks/TASK-0276-parallelism-v1-deterministic-threadpools-policy-contract.md
  - label: "Policy / capability matrix direction"
    target: tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - label: "NexusGfx SDK (compute/present consumers)"
    target: tasks/TRACK-NEXUSGFX-SDK.md
  - label: "NexusMedia SDK (audio/video/image ML consumers)"
    target: tasks/TRACK-NEXUSMEDIA-SDK.md
  - label: "NexusGame SDK (realtime ML-assisted features)"
    target: tasks/TRACK-NEXUSGAME-SDK.md
  - label: "Search v2 lexical baseline (no ML in v2.1)"
    target: tasks/TASK-0153-search-v2-backend-host-index-ranking-analyzers-sources.md
  - label: "Search v2.1 semantic-lite (deterministic, no embeddings)"
    target: tasks/TASK-0213-search-v2_1a-host-semvec-tags-expansion-hybrid.md
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

For image/video/tensor style workflows, `NexusInfer` should align with the layer model, resource model,
sync/lifetime model and capability vocabulary in the "Design notes" section of `tasks/TRACK-NEXUSGFX-SDK.md`.

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

## Design notes (merged from docs/architecture/inference, 2026-11-02)

Research vocabulary for the future NexusInfer RFC, which fixes the normative names. Nothing here is a contract yet.

### Parameter vocabulary — never one "model size" number

| Term | Meaning |
|------|---------|
| total | all learned parameters in the checkpoint |
| loaded | parameters in any accessible memory now |
| resident | parameters inside the executor's fast working budget now |
| active | parameters used for this token/request |
| effective | practical footprint of the selected profile after streaming, skipping or nesting |

- A profile may claim a small *effective* footprint only if it also states what is *loaded* or *streamed* elsewhere. Profiles report all five.
- Active ≠ loaded: an expert-routed (MoE) model activates few experts per token but keeps the whole pool loaded; plan routing cost apart from active compute.

### Runtime profile fields

| Field | Candidate values |
|-------|------------------|
| `executor_class` | `cpu_ref` (deterministic proof path) · `cpu_optimized` · `npu` · `gfx_compute` |
| `weight_precision`, `activation_precision`, `k_cache_precision`, `v_cache_precision` | `bf16` · `sfp8` · `int8` · `int4` · `mixed` · `backend_native` — four independent fields |
| `kv_policy` | `none` · `full` · `windowed` · `shared` · `quantized` · `offloaded`, combinable (`shared+quantized`) |
| `residency_policy` | `core_resident` · `modality_optional` · `layer_streamed` · `storage_backed_assist` |
| `modality_policy` | `text_only` · `text_image` · `text_audio` · `multimodal_full` |
| `submodel_tier` | `tier_min` · `tier_balanced` · `tier_quality` · `tier_custom` |
| `power_profile` | `fixed` · `adaptive` · `frugal` · `normal` · `burst` (QoS hint) |
| `deadline_class` | `best_effort` · `interactive` · `realtime_assist` · `background_batch` |
| `tolerance_class` | exact · tolerance-bounded |

- Tier and profile changes are visible in policy/profile metadata. A thermal/battery downgrade (smaller tier, lower precision, text-only, CPU fallback, shorter context) is an explicit profile transition under QoS, never a backend surprise.
- KV pages, when externally materialized, ride the VMO/filebuffer data plane like weights.

### Reference profiles (proof targets)

| Profile | For | Typical settings |
|---------|-----|------------------|
| `cpu_ref` | host/QEMU proofs, API bring-up | `cpu_ref`, bf16 or host-friendly weights, KV full/windowed, text_only, power fixed, best_effort/interactive |
| `low_ram` | constrained devices | lower weight precision, optional modalities, layer streaming, KV shared/quantized |
| `low_latency` | speech or responsive UI assistant | smaller tier, prefill-efficient, optional shared KV, modalities loaded on demand |
| `long_context` | document/video/audio summarization | local/global-attention model family, KV windowed/shared, optionally quantized/offloaded |
| `thermal_safe` | sustained use | adaptive power, downgrade path to smaller tier or CPU, fewer modalities |
| `quality_burst` | short high-quality turn | larger tier, higher precision, bounded burst budget |

Proof policy: each extracted task names which profile parts are proven by exact host tests, tolerance-bounded host tests, QEMU functional markers, or backend capability checks. `cpu_ref`: exact or tightly bounded. `npu low_ram`: functional equivalence + bounded tolerance. `long_context shared+quantized`: memory gate and output-quality gate kept separate.

### Techniques → what the contract must carry

The first nine rows are documented in public material of an open on-device model family; the last five are candidate runtime policies.

| Technique | What it does | Contract consequence |
|-----------|--------------|----------------------|
| Per-layer embeddings (PLE) caching | per-layer embedding blocks live in fast local storage, injected as each layer runs | parameter classes core_resident / layer_streamed / optional_modality; capability for storage-assisted injection |
| Effective parameters | runtime footprint below the stored total | profiles carry total and effective (plus loaded, active) |
| Conditional parameter loading | audio/vision blocks stay unloaded for text-only work | explicit modality mask; model = independently managed parameter groups, not one blob |
| Hybrid local/global attention | sliding-window layers interleaved with sparse global layers | KV policy distinguishes windowed / global / mixed — not a `use_cache` bool |
| KV cache sharing | upper layers reuse K/V of earlier compatible layers; faster prefill | `shared` is a named policy, not hidden backend behaviour |
| Activation quantization | activations in lower precision, maybe only some subgraphs/backends | own precision field; tests state exact / tolerance-bounded / backend-specific per op family |
| Weight quantization profiles | one family shipped at bf16 / sfp8 / 4-bit | named weight precision class; does not imply every tensor tolerates it |
| Expert routing (MoE) | few experts active, full pool loaded | loaded and active reported separately |
| Nested submodels (MatFormer) | a large model contains smaller runnable subnetworks | named tiers; switching is an explicit runtime feature |
| Online KV compression (TurboQuant-like: PolarQuant-style transform + scalar quantization, then 1-bit QJL residual correction) | low-distortion KV vectors; KV is the long-context memory wall | room for `quantized`, `quantized_turboquant_like` and future tags; the ABI names the goal, not a brand |
| Asymmetric K/V quantization | K is more sensitive than V | independent K and V precision |
| KV offloading / tiering | old KV pages move to slower RAM or storage | tiered residency policy, only with explicit bounds and reclaim — no unbounded paging loop |
| Profile switching under pressure | runtime changes class mid-session | transition policy governed by QoS (see above) |
| Tensor-class-sensitive quantization | embeddings, routers, encoders kept at higher precision | per-tensor-class policy |

Tensor classes: `embedding`, `attention_q`, `attention_k`, `attention_v`, `attention_out`, `mlp`, `router`, `vision_encoder`, `audio_encoder`, `kv_cache_k`, `kv_cache_v`.

### Backend capabilities (generic terms only)

`supports_int4_weights`, `supports_int8_dot`, `supports_int8_activations`, `supports_quantized_kv`, `supports_shared_kv`, `supports_fast_storage_parameter_streaming`, `supports_image_tensor_interop`, `supports_async_copy`. Planning and fallback run on these; matrix-tile sizes, warp reductions and graph-capture semantics stay out of the portable surface.

### Rust shape (input for TASK-0284B and after)

- Newtypes on the hot path, never raw `u32`/`usize`/strings: `SessionId`, `GraphId`, `ModelId`, `TensorId`, `TensorSliceId`, `FenceId`, `QueueId`, `ExecutorId`, `ProfileId`, `VmoHandle`, `FilebufferHandle`; value wrappers `Shape`, `DType`, `Layout`, `ByteLen`, `ByteOffset`, `TokenCount`, `LayerIndex`. `#[repr(transparent)]` where ABI-visible, one construction authority, untrusted integers validated before conversion, no casual from-raw constructors (TASK-0281 direction).
- Tensor type states `Tensor<S>`, `S` ∈ `HostOwned` (exclusive mutable) · `ReadOnlyMapped` (immutable view) · `ExecutorOwned` (moved into the executor domain) · `InFlight` (only fence completion returns access).
- Model residency states: `Unloaded` · `MetadataOnly` · `PartiallyLoaded` · `Resident` · `Suspended` — conditional loading in types, not bool flags.
- Metadata apart from bytes: `TensorDesc { shape, dtype, layout, len }` on the control plane; `TensorStorage::{Vmo, Filebuffer} { handle, offset, len }` on the data plane. Slices, borrowed views and ownership transfer over copies into `Vec<u8>`.
- Executor: `fn submit(&mut self, s: Self::Submission) -> Result<Self::Fence, SubmitError>`; one mutable owner per queue, submit moves ownership in, fence completion returns it. No global shared executor, no callback mutation, no hidden worker pools.
- `Send` not `Sync`: `Tensor<HostOwned>`, `Fence`, `Submission`, `SessionCommand`, `ExecutorTicket`. Neither: executor-local scratch allocators, CPU decode caches, single-owner queue internals, thread-bound encoders. `Sync`: immutable `TensorDesc`, model metadata, quantization tables, profile catalogs. `unsafe impl Send/Sync` only when auto-derive is impossible and the invariant is written next to it.
- No `Arc<Mutex<_>>` / `Arc<RwLock<_>>` / mutable global caches in the submit/decode path; profiles are exhaustive enums internally even if the wire uses stable strings.

## See also

- Networking-style bulk patterns: `tasks/TRACK-NEXUSNET-SDK.md`
- Zero-copy app platform: `tasks/TRACK-ZEROCOPY-APP-PLATFORM.md`
- Keystone dependency: `tasks/TRACK-KEYSTONE-GATES.md` (VMO proofs)
