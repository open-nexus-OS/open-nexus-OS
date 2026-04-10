# NexusInfer Techniques Catalog

**Created**: 2026-04-10  
**Owner**: @runtime @ui  
**Status**: Active guidance for `tasks/TRACK-NEXUSINFER-SDK.md`; not itself a wire/API contract

---

## Purpose

This document records the **techniques and algorithms** that matter for a future
Open Nexus OS on-device inference stack so later RFCs/tasks do not have to
reconstruct the design space from scratch.

This page answers:

- which efficiency techniques are **confirmed** in Google's open model lineage,
- which techniques are **candidate runtime strategies** for NexusInfer,
- what each technique **actually does**,
- and what the technique implies for our **runtime contract**, **testing**, and
  **hardware abstraction**.

Rule: **do not treat this page as the runtime contract**. The future RFC defines
normative API/IDL/profile fields. This page exists to make that RFC precise.

---

## Hardware-independence rule

NexusInfer must remain **hardware-agnostic by default**:

- no CUDA- or Tensor-Core-specific contract surfaces,
- no assumption that the execution backend exposes Nvidia-style warp, stream, or
  kernel-launch semantics,
- no assumption that the future GPU path is identical to TPU, NPU, or DSP paths,
- and no requirement that compute goes through the GPU at all.

This matters because Open Nexus OS is more likely to target:

- **CPU reference execution** first,
- **future NPU executors** through `TRACK-DRIVERS-ACCELERATORS`,
- and potentially **Imagination-backed GPU compute interop** behind
  `TRACK-NEXUSGFX-SDK`, not CUDA.

So every technique below must be phrased in terms of:

- parameter residency,
- tensor/control/data movement,
- cache policy,
- quantization policy,
- and executor capabilities,

not vendor-specific APIs.

---

## Terms (must stay distinct)

- **Total parameters**: all learned parameters present in the full model.
- **Loaded parameters**: parameters currently loaded into accessible memory.
- **Resident parameters**: parameters currently held in the active fast memory
  budget for the executor.
- **Active parameters**: parameters actually used for a token/request.
- **Effective parameters**: the practical parameter footprint required for a
  specific runtime mode, after techniques such as PLE, parameter skipping, or
  nested submodels are applied.

These terms must not be collapsed into a single "model size" number in later
tasks or RFCs.

---

## Confirmed upstream techniques

These are documented in public Google material for the Gemma 3n / Gemma 4
family and should be treated as **real upstream patterns**, not speculation.

### 1. Per-Layer Embeddings (PLE) / PLE caching

#### What happens

- Each decoder layer has an associated embedding-like parameter block.
- These PLE parameters do not need to stay in the accelerator-resident working
  set all the time.
- The data can be prepared or cached in fast local storage and injected as each
  layer executes.

#### Why it matters

- Reduces active accelerator memory footprint.
- Allows a model with larger total parameters to behave like a smaller
  "effective" model during inference.

#### NexusInfer implication

- The runtime needs a **parameter residency model**; "weights loaded" is not a
  sufficient abstraction.
- The runtime contract should support parameter classes such as:
  `core_resident`, `layer_streamed`, and `optional_modality`.
- The executor profile must be able to say whether fast-storage-assisted
  parameter injection is supported.

### 2. Effective parameters

#### What happens

- The model's useful runtime footprint is described by a lower effective count
  than the total stored parameter count.
- This is not just marketing; it reflects a real runtime property arising from
  parameter skipping, PLE, or nested execution.

#### Why it matters

- Capacity planning based only on total checkpoint size is misleading.
- Scheduler, battery, and thermal policy need a number closer to "active
  working set under this profile".

#### NexusInfer implication

- Profiles must carry both `total_parameters` and `effective_parameters`.
- Planning docs must additionally track `loaded_parameters` and
  `active_parameters`.

### 3. Conditional parameter loading / parameter skipping

#### What happens

- Audio or vision parameter blocks can remain unloaded when a request is
  text-only.
- The runtime selectively loads or skips modality-specific model components.

#### Why it matters

- Avoids paying multimodal memory cost for text-only work.
- Fits battery- and thermally-constrained devices.

#### NexusInfer implication

- Session/profile metadata must include an explicit **modality mask**.
- Model loading must be split into independently managed parameter groups, not a
  single monolithic blob.

### 4. Hybrid local/global attention

#### What happens

- The model alternates or interleaves **local sliding-window attention** with
  less frequent **global attention** layers.
- Local layers keep a short context window; global layers preserve long-range
  context.

#### Why it matters

- Reduces KV-cache explosion for long contexts.
- Keeps long-context inference viable without forcing every layer to store full
  global history.

#### NexusInfer implication

- KV policy cannot be a single bool like `use_cache`.
- Runtime docs should distinguish:
  - `windowed_kv`,
  - `global_kv`,
  - `mixed_local_global_kv`.

### 5. KV cache sharing

#### What happens

- Instead of every layer materializing fully distinct K/V state, upper layers
  reuse K/V derived from earlier compatible layers.
- This especially improves prefill efficiency and long multimodal prompts.

#### Why it matters

- Less memory pressure.
- Faster time-to-first-token / faster prompt processing.

#### NexusInfer implication

- KV cache policy must include **shared KV** as a first-class option.
- The runtime must describe sharing at the level of **policy**, not as hidden
  backend behavior.

### 6. Activation quantization

#### What happens

- Intermediate activations are quantized to lower precision during inference.
- This is distinct from weight quantization and may be supported only for some
  subgraphs or backends.

#### Why it matters

- Reduces memory bandwidth and active working set.
- Can help mobile/on-device latency and power if numerically safe.

#### NexusInfer implication

- Quantization policy must be split into:
  - weight precision,
  - activation precision,
  - KV precision.
- Tests must state whether an op family is exact, tolerance-bounded, or backend
  specific.

### 7. Weight quantization profiles

#### What happens

- The same model family is published in or intended to run with multiple
  precisions, such as BF16, SFP8, or 4-bit variants.

#### Why it matters

- Weight precision is the most obvious but not the only efficiency lever.
- Model size alone does not predict runtime behavior once KV and activations are
  included.

#### NexusInfer implication

- The runtime profile needs a named **weight precision class**.
- The profile must not imply that all tensors inside the model share the same
  safe quantization level.

### 8. Mixture-of-Experts (MoE): active vs loaded distinction

#### What happens

- Only a subset of expert parameters are active per token.
- However, the full expert pool may still need to be loaded to keep routing and
  latency acceptable.

#### Why it matters

- "Only 4B active" does not mean "fits like a 4B model".
- Planning and policy need both active and loaded numbers.

#### NexusInfer implication

- Task/RFC docs must never equate active parameters with real memory residency.
- Executor profiles need to distinguish **routing cost** from **active compute
  cost**.

### 9. MatFormer / nested submodels (Google lineage; especially Gemma 3n)

#### What happens

- A larger model contains nested smaller subnetworks that can run independently
  or as intermediate profiles.

#### Why it matters

- Lets a runtime trade quality, latency, and energy without shipping separate
  models.

#### NexusInfer implication

- Profile switching between submodels should be treated as an explicit runtime
  feature, not a hidden backend heuristic.
- Future contracts should support named submodel tiers or execution classes.

---

## Candidate runtime techniques for NexusInfer

These techniques are highly relevant for a local-first system, but should be
recorded as **candidate runtime policies** until a task/RFC fixes them.

### 10. TurboQuant-like KV compression

#### What happens

- KV cache vectors are compressed using a specialized low-distortion online
  vector quantization pipeline.
- In the public Google Research description, the pipeline is two-stage:
  - a **PolarQuant-style** transform + scalar quantization stage,
  - followed by a **1-bit QJL residual correction** stage.

#### Why it matters

- KV cache often becomes the real long-context memory wall.
- This can matter more than further shrinking already-quantized weights.

#### NexusInfer implication

- KV policy should have room for:
  - `kv_quantized`,
  - `kv_quantized_turboquant_like`,
  - and future algorithm tags.
- The contract should describe the **goal** ("online low-distortion KV
  compression") without baking in a single brand name as ABI.

### 11. Asymmetric K/V quantization

#### What happens

- K and V are not treated identically.
- Practical systems and experiments often show that **K is more sensitive** than
  **V**, so mixed precision may be preferable.

#### Why it matters

- A simple single `kv_bits=4` policy can be too coarse.
- Better quality/latency tradeoffs may require separate K and V treatment.

#### NexusInfer implication

- KV profile fields should be able to describe **K precision** and **V
  precision** independently.

### 12. KV offloading / tiered storage

#### What happens

- Inactive or older KV segments are moved out of the fastest memory tier to
  slower RAM or storage-backed tiers.

#### Why it matters

- Enables longer context on constrained devices.
- Useful when the device has fast local storage but limited DRAM/VRAM.

#### NexusInfer implication

- Future runtime profiles should allow a **tiered residency policy** for KV.
- This is compatible with Open Nexus design only if it remains bounded and
  explicit; no hidden unbounded paging loops.

### 13. Runtime profile switching under thermal or battery pressure

#### What happens

- The runtime changes execution class mid-session or per request:
  - smaller submodel,
  - lower precision,
  - text-only modality,
  - CPU fallback,
  - reduced context strategy.

#### Why it matters

- Real devices do not stay in one thermal/power state.

#### NexusInfer implication

- Power/thermal adaptation should be a **profile transition policy** governed by
  QoS, not an opaque backend surprise.

### 14. Tensor-class-sensitive quantization

#### What happens

- Some tensors or tensor families are much more quality-sensitive than others.
- Practical implementations often avoid aggressive quantization for selected
  embeddings, routing tensors, or modality-specific blocks.

#### Why it matters

- Treating every tensor identically can lose quality unnecessarily.

#### NexusInfer implication

- The future exporter/runtime should permit **tensor class policies**, not just
  one bit-width for the whole checkpoint.

---

## What must become normative later

The future RFC should define these explicitly:

1. **Profile vocabulary**
   - `cpu_ref`, `low_ram`, `low_latency`, `long_context`, `thermal_safe`,
     `quality_burst`
2. **Residency vocabulary**
   - total, loaded, resident, active, effective
3. **Quantization vocabulary**
   - weights, activations, K cache, V cache
4. **KV vocabulary**
   - full, shared, windowed, quantized, offloaded
5. **Submodel vocabulary**
   - fixed model, nested submodel, optional modality blocks

---

## Anti-drift rules

- Do **not** document NexusInfer as "CUDA-compatible first"; the contract must
  remain portable to CPU, NPU, and future Imagination/NexusGfx execution.
- Do **not** equate "effective parameters" with loaded memory or with active
  per-token compute.
- Do **not** reduce quantization to a single checkpoint label like `q4`; track
  weights, activations, and KV separately.
- Do **not** hide long-context behavior in backend magic; KV policy must be
  named and testable.
- Do **not** force Search v2.1 or other non-ML tasks to retroactively adopt ML
  scope.

---

## Canonical follow-on documents

- Track: `tasks/TRACK-NEXUSINFER-SDK.md`
- Runtime profiles: `docs/architecture/nexusinfer-runtime-profiles.md`
- Rust design: `docs/architecture/nexusinfer-rust-design.md`
