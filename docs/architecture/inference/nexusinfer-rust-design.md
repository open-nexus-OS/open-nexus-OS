# NexusInfer Rust Design Notes

**Created**: 2026-04-10  
**Owner**: @runtime  
**Status**: Active design guidance for userspace/runtime work

---

## Purpose

This page records how Rust should be used to make NexusInfer:

- **safer**,
- **more deterministic**,
- **zero-copy-friendly**,
- and **less hardware-specific**

without turning the runtime into a pile of untyped handles and shared mutable
state.

It builds on the existing Open Nexus direction for:

- newtypes,
- ownership transfer,
- `Send`/`Sync` discipline,
- and bounded APIs.

This document is guidance for future userspace/runtime tasks, not a complete API
spec.

---

## Design rules

### 1. Prefer ownership and type states over runtime bookkeeping

Use the type system to model:

- who owns a tensor/buffer,
- whether a buffer is writable,
- whether a tensor is in-flight on an executor,
- and whether a handle is CPU-local or transferable.

This reduces:

- double-submit bugs,
- use-after-free style mistakes,
- accidental aliasing,
- and hidden synchronization costs.

### 2. Prefer generic executor traits over vendor APIs

Rust interfaces should describe:

- submit,
- map,
- fence wait,
- import/export,
- capability query,

not CUDA streams, Tensor Cores, or vendor queue objects.

Future Imagination GPU or NexusGfx compute backends should be able to implement
the same traits as CPU or NPU executors.

### 3. Keep userspace crates `unsafe`-minimal

For host-first/runtime crates:

- prefer `#![forbid(unsafe_code)]`,
- isolate unavoidable low-level mapping glue behind narrow adapter layers,
- and document any `unsafe` invariants in-line when they are truly required.

### 4. Avoid blanket shared mutability in hot paths

Avoid defaulting to:

- `Arc<Mutex<_>>`
- `Arc<RwLock<_>>`
- shared mutable global caches

for the core submit/decode path.

Instead prefer:

- move-only ownership,
- read-only shared metadata,
- executor-local scratch state,
- message passing,
- and explicit fences.

---

## Newtypes to require early

The runtime should not pass raw `u32`, `usize`, or opaque strings through the
hot path when they represent distinct concepts.

Suggested newtypes:

- `SessionId`
- `GraphId`
- `ModelId`
- `TensorId`
- `TensorSliceId`
- `FenceId`
- `QueueId`
- `ExecutorId`
- `ProfileId`
- `VmoHandle`
- `FilebufferHandle`

Suggested wrappers for logical categories:

- `Shape`
- `DType`
- `Layout`
- `ByteLen`
- `ByteOffset`
- `TokenCount`
- `LayerIndex`

Rules:

- use `#[repr(transparent)]` for ABI-visible wrappers when relevant,
- centralize construction authority,
- validate untrusted integers before conversion,
- do not expose "mint from raw integer" constructors casually.

---

## Ownership state patterns

### Tensor payload states

Use type states or distinct wrapper types for lifecycle:

```rust
struct HostOwned;
struct ReadOnlyMapped;
struct ExecutorOwned;
struct InFlight;

struct Tensor<S> {
    id: TensorId,
    desc: TensorDesc,
    storage: TensorStorage,
    _state: core::marker::PhantomData<S>,
}
```

Meaning:

- `Tensor<HostOwned>`: CPU/runtime has exclusive mutable ownership
- `Tensor<ReadOnlyMapped>`: safe immutable view
- `Tensor<ExecutorOwned>`: ownership transferred into executor domain
- `Tensor<InFlight>`: submitted; only fence completion can return usable access

Why:

- prevents CPU writes after submit,
- prevents double-submit,
- makes fence completion the recovery point for ownership.

### Model residency states

Large model objects may also need states:

- `Unloaded`
- `MetadataOnly`
- `PartiallyLoaded`
- `Resident`
- `Suspended`

This makes "effective parameters" and conditional loading explicit in code,
rather than hidden in boolean flags.

---

## Storage abstractions

### Separate metadata from payload

Do not couple tensor metadata to tensor bytes in one giant mutable struct.

Prefer:

```rust
struct TensorDesc {
    shape: Shape,
    dtype: DType,
    layout: Layout,
    len: ByteLen,
}

enum TensorStorage {
    Vmo { handle: VmoHandle, offset: ByteOffset, len: ByteLen },
    Filebuffer { handle: FilebufferHandle, offset: ByteOffset, len: ByteLen },
}
```

This matches the project's hybrid control/data plane:

- Cap'n Proto for `TensorDesc` and references
- VMO/filebuffer for bulk bytes

### Zero-copy rule

The runtime should prefer:

- slices,
- borrowed read-only views,
- ownership transfer,

over copying tensor payloads into owned `Vec<u8>` blobs in service hot paths.

---

## `Send` and `Sync` guidance

The runtime should be conservative and explicit.

### Likely `Send` but not `Sync`

- `Tensor<HostOwned>`
- `Fence`
- `Submission`
- `SessionCommand`
- `ExecutorTicket`

Reason:

- transferable between workers or queues
- not meant to be shared mutably

### Likely neither `Send` nor `Sync`

- executor-local scratch allocators
- CPU-local decode caches
- queue internals with single-owner mutation
- backend command encoders tied to one thread/context

### Likely `Sync`

- immutable `TensorDesc`
- immutable model metadata
- static quantization tables
- readonly profile catalogs

### Rule for unsafe impls

Only use `unsafe impl Send/Sync` when:

- auto-derive is impossible,
- the invariant is small and auditable,
- and the invariant is written next to the impl.

Do not use `unsafe impl Send/Sync` as a shortcut around ownership design.

---

## Queue and submit model

Preferred pattern:

- each queue/submitter has **single mutable ownership**
- work submission **moves** ownership into the executor path
- fence completion returns exclusive ownership back

Example shape:

```rust
trait Executor {
    type Submission;
    type Fence;

    fn submit(
        &mut self,
        submission: Self::Submission,
    ) -> Result<Self::Fence, SubmitError>;
}
```

This is preferable to:

- global executor objects shared everywhere,
- ad-hoc callback mutation,
- hidden internal worker pools with unclear ownership.

---

## Rust mapping for runtime profiles

Profiles should be typed internally, even if the wire format later uses stable
strings.

Suggested enums:

```rust
enum ExecutorClass { CpuRef, CpuOptimized, Npu, GfxCompute }
enum KvPolicy { None, Full, Windowed, Shared, Quantized, Offloaded }
enum PowerProfile { Fixed, Adaptive, Frugal, Normal, Burst }
enum DeadlineClass { BestEffort, Interactive, RealtimeAssist, BackgroundBatch }
enum ModalityPolicy { TextOnly, TextImage, TextAudio, MultimodalFull }
```

Why:

- keeps internal logic exhaustive and compiler-checked,
- prevents stringly-typed drift between docs, runtime, and tests.

---

## Generic capability model

Backends should advertise generic capabilities:

```rust
struct BackendCapabilities {
    supports_int4_weights: bool,
    supports_int8_activations: bool,
    supports_quantized_kv: bool,
    supports_shared_kv: bool,
    supports_async_copy: bool,
    supports_fast_storage_streaming: bool,
    supports_image_tensor_interop: bool,
}
```

This allows:

- CPU, NPU, and future Imagination/NexusGfx paths to share the same runtime
  planning layer,
- capability-driven fallback,
- and hardware independence in the public contract.

It also avoids embedding assumptions like:

- Tensor Core matrix sizes,
- CUDA graphs,
- warp-level reductions,
- or specific vendor memory semantics.

---

## Relationship to existing Open Nexus work

This page follows existing repo direction:

- newtype construction authority (`TASK-0281` direction),
- ownership-based DMA buffer lifecycle (`TASK-0284`),
- `Send`/`Sync` caution from `docs/architecture/16-rust-concurrency-model.md`,
- zero-copy data plane from `TASK-0031` and RFC-0005.

NexusInfer should extend those ideas into userspace inference rather than
inventing a separate ad-hoc model runtime style.

---

## Anti-patterns to avoid

- raw integer IDs crossing every layer without wrappers
- executor APIs that mention CUDA, Tensor Cores, or other vendor primitives
- `Arc<Mutex<Vec<u8>>>` as the default tensor storage shape
- copying payloads into Cap'n Proto messages
- hidden background threads mutating shared model state
- fence-less ownership transfer
- "temporary" unsafe send/sync impls without documented invariants

---

## Related

- Track: `tasks/TRACK-NEXUSINFER-SDK.md`
- Techniques catalog: `docs/architecture/nexusinfer-techniques.md`
- Runtime profiles: `docs/architecture/nexusinfer-runtime-profiles.md`
- DMA ownership prototype task: `tasks/TASK-0284-userspace-dmabuffer-ownership-v1-prototype.md`
