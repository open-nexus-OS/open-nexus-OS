# NexusInfer Runtime Profiles

**Created**: 2026-04-10  
**Owner**: @runtime @ui  
**Status**: Active guidance for future NexusInfer RFC/task extraction

---

## Purpose

This document defines the **runtime vocabulary** that NexusInfer should use so
later tasks and RFCs can specify behavior without ambiguity.

This page is about:

- **what knobs exist**,
- **what they mean**,
- **which combinations are valid**,
- and **how they stay hardware-independent**.

This page is **not** the final wire/API contract; it is the blueprint for that
contract.

---

## Design rule: describe capabilities, not vendors

NexusInfer profiles must be written against **generic execution capabilities**,
not vendor marketing names.

Allowed examples:

- `supports_int8_dot`
- `supports_int4_weights`
- `supports_quantized_kv`
- `supports_shared_kv`
- `supports_fast_storage_parameter_streaming`
- `supports_image_tensor_interop`
- `supports_async_copy`

Disallowed as normative contract terms:

- "requires CUDA"
- "requires Tensor Cores"
- "requires warp-level MMA"
- "requires CUDA graphs"

Future Imagination GPU or NexusGfx compute interop is welcome, but it must map
to generic capabilities instead of leaking hardware-specific terms into app or
service contracts.

---

## Core dimensions

Every runtime profile should be understood as a combination of the dimensions
below.

### 1. Executor class

- `cpu_ref`: deterministic CPU reference path; the default proof surface.
- `cpu_optimized`: CPU path optimized for local performance.
- `npu`: future accelerator executor under `TRACK-DRIVERS-ACCELERATORS`.
- `gfx_compute`: optional future compute path through `TRACK-NEXUSGFX-SDK`.

### 2. Precision classes

Tracked independently:

- **weights**
- **activations**
- **K cache**
- **V cache**

Example values:

- `bf16`
- `sfp8`
- `int8`
- `int4`
- `mixed`
- `backend_native`

### 3. KV policy

- `none`
- `full`
- `windowed`
- `shared`
- `quantized`
- `offloaded`
- combinations such as `shared+quantized`

### 4. Residency policy

- `core_resident`
- `modality_optional`
- `layer_streamed`
- `storage_backed_assist`

### 5. Modality policy

- `text_only`
- `text_image`
- `text_audio`
- `multimodal_full`

### 6. Thermal and power behavior

- `fixed`
- `adaptive`
- `frugal`
- `normal`
- `burst`

### 7. Deadline class

- `best_effort`
- `interactive`
- `realtime_assist`
- `background_batch`

---

## Required profile fields

The future RFC should define at least these fields for a runtime profile:

| Field | Meaning |
|-------|---------|
| `executor_class` | CPU ref, NPU, or other generic executor |
| `weight_precision` | Precision for static weights |
| `activation_precision` | Precision for intermediate activations |
| `k_cache_precision` | Precision for K cache |
| `v_cache_precision` | Precision for V cache |
| `kv_policy` | Full/shared/windowed/quantized/offloaded |
| `residency_policy` | Resident vs streamed vs optional modality parameters |
| `modality_policy` | Which modality blocks are active/loaded |
| `submodel_tier` | Full vs nested/smaller submodel |
| `power_profile` | Frugal/Normal/Burst style QoS hint |
| `deadline_class` | How aggressive latency budgets are |
| `tolerance_class` | Exact vs tolerance-bounded validation |

---

## Residency model

To avoid misleading capacity planning, every profile should report:

- `total_parameters`
- `loaded_parameters`
- `resident_parameters`
- `active_parameters`
- `effective_parameters`

Recommended interpretation:

- **Total**: full checkpoint content
- **Loaded**: all parameters currently loaded somewhere accessible
- **Resident**: parameters occupying the fast execution budget right now
- **Active**: parameters actually used for this request/token
- **Effective**: practical footprint required for the selected profile

Rule: a profile may claim a small **effective** footprint only if it also
states what is merely **loaded** or **streamed** elsewhere.

---

## Reference runtime profiles

These profiles are guidance targets for tasks and proofs.

### `cpu_ref`

Purpose:

- deterministic host/QEMU proofs
- minimal API bring-up
- no hardware dependency

Typical settings:

- executor: `cpu_ref`
- weights: `bf16` or bounded host-friendly format
- activations: exact or tolerance-bounded documented behavior
- KV: `full` or `windowed`
- modality: usually `text_only`
- power: `fixed`
- deadline: `best_effort` or `interactive`

### `low_ram`

Purpose:

- fit on constrained devices

Typical settings:

- lower weight precision
- optional modality loading
- PLE or streamed parameter support
- KV `shared` or `quantized`

### `low_latency`

Purpose:

- speech assistant or responsive UI assistant

Typical settings:

- smaller submodel tier
- aggressive prefill efficiency
- optional KV sharing
- modality blocks loaded only when needed

### `long_context`

Purpose:

- document/video/audio summarization and larger search contexts

Typical settings:

- hybrid local/global attention compatible model family
- `windowed` and/or `shared` KV
- optional `quantized` or `offloaded` KV

### `thermal_safe`

Purpose:

- sustained on-device usage without runaway heat

Typical settings:

- adaptive power profile
- downgrade path to smaller submodel or CPU fallback
- reduced modality load

### `quality_burst`

Purpose:

- short, high-quality interaction when thermally affordable

Typical settings:

- larger active tier
- higher precision
- shorter bounded burst budget

---

## Submodel and tier policy

Nested-model techniques such as MatFormer-like execution should map to explicit
tiers:

- `tier_min`
- `tier_balanced`
- `tier_quality`
- `tier_custom`

Rule:

- Tier changes must be **observable in policy/profile metadata**.
- The runtime must not silently swap in a different submodel without the policy
  layer knowing which profile is active.

---

## KV policy notes

KV cache behavior must be explicit because it dominates long-context cost.

### `full`

- Standard KV retention
- simplest behavior
- high memory cost

### `windowed`

- local attention keeps only bounded recent history
- good fit for hybrid local/global architectures

### `shared`

- later layers reuse compatible earlier K/V
- reduces memory and often prefill cost

### `quantized`

- K/V stored in lower precision
- should support asymmetric K/V precision

### `offloaded`

- some KV pages live in slower memory tiers
- only valid with explicit bounds and reclaim policy

---

## Tensor classes

The runtime should not assume all tensors are equally quantizable.

Suggested tensor classes:

- `embedding`
- `attention_q`
- `attention_k`
- `attention_v`
- `attention_out`
- `mlp`
- `router`
- `vision_encoder`
- `audio_encoder`
- `kv_cache_k`
- `kv_cache_v`

Why this matters:

- some classes are much more quality-sensitive,
- K and V often need different treatment,
- modality encoders may need separate policies.

---

## Control plane and data plane mapping

Profiles are control-plane metadata. Bulk tensors remain on the data plane.

### Control plane

Cap'n Proto should carry:

- session/profile identifiers
- profile fields
- tensor metadata
- offsets/slices
- fence/QoS IDs
- validation errors

### Data plane

VMO/filebuffer should carry:

- weights
- tensor payloads
- image/audio frame buffers
- KV pages when externally materialized

Rule: profile choice must not cause the runtime to start embedding large tensor
payloads in Cap'n Proto messages.

---

## Proof policy

Every extracted task should name which parts of a profile are proven by:

- **exact host tests**
- **tolerance-bounded host tests**
- **QEMU functional markers**
- **backend capability checks**

Examples:

- `cpu_ref`: exact or tightly bounded
- `npu low_ram`: functional equivalence + bounded tolerance
- `long_context shared+quantized`: memory and output quality gates separated

---

## Anti-drift rules

- Never use "small model" as a substitute for a full runtime profile.
- Never use a vendor runtime name as the profile contract.
- Never treat KV behavior as an implementation detail.
- Never treat active parameters as loaded memory.
- Never make CUDA/TPU assumptions part of the portable API surface.

---

## Related

- Track: `tasks/TRACK-NEXUSINFER-SDK.md`
- Techniques catalog: `docs/architecture/nexusinfer-techniques.md`
- Rust design: `docs/architecture/nexusinfer-rust-design.md`
