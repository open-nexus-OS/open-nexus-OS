# ADR-0018: DriverKit ABI Versioning and Stability

## Status

Proposed

## Context

Open Nexus OS uses a microkernel-style architecture where **device-class drivers** live in userspace. To reduce kernel complexity and TCB size while keeping performance high, we want a stable, narrow cross-device boundary:

- **Kernel**: capability-gated MMIO/IRQ/DMA primitives, VMOs, scheduling/QoS hints (no device-specific policy).
- **Userspace driver service**: device programming, firmware protocol, command validation, reset/recovery.
- **DriverKit (shared libraries) + SDKs**: common concepts like queues, fences, buffers, budgets/backpressure, tracing hooks.

This implies we need a clear rule-set for what “stable” means across:

- userspace ↔ userspace (SDK ↔ driver service),
- userspace ↔ kernel (capabilities/VMO/fence syscalls),
- and OS-lite ↔ OS (feature gates and deterministic proofs).

Without an explicit ABI stability policy, the project risks:

- duplicated ad-hoc “driver APIs” per device category,
- drift between SDK/driver services,
- breaking changes without audit/testing gates,
- and more kernel surface than necessary.

## Decision

We standardize on a **DriverKit ABI v1** with strict versioning and stability rules.

### D1. Define the “DriverKit ABI” boundary

DriverKit ABI is the minimal, stable boundary shared across device-class services (GPU/NPU/VPU/Audio/Camera/ISP/Storage/Networking accelerators):

- **Buffer handles**: VMO/filebuffer, slices, budgets.
- **Submit model**: queue submission with bounded in-flight work.
- **Synchronization**: timeline fences, waitsets, deadlines.
- **Fault/reset**: per-client kill + device reset semantics, audited.

Everything device-specific stays outside the ABI:

- register/MMIO programming,
- firmware protocols,
- command encoding formats,
- and device-specific validation rules.

### D2. Versioning

- **Major**: breaking on-wire or syscall-visible changes.
- **Minor**: additive (new optional messages/fields/ops), backward compatible.
- **Patch**: bug fixes and clarifications.

### D3. Compatibility rules (v1)

- **No breaking changes** to v1 messages/syscalls once shipped in an OS release.
- Additive changes must be:
  - bounded (size limits),
  - deterministic,
  - and negotiated via explicit version fields.
- “Feature bits” are allowed only if:
  - they are stable and documented,
  - default behavior is safe and deterministic.

### D4. Representation rules

- OS-lite prefers **small, versioned byte frames** as the authoritative on-wire contract for v1.
- Higher-level schema tooling (Cap’n Proto, etc.) may exist for documentation/testing, but must not become the only contract without an explicit repo decision.

### D5. Security and authority rules

- All privileged device operations are capability-gated; no ambient device access.
- Policy authority remains single-source (policyd decides; kernel enforces held rights).
- No capability transfer “over the network boundary” for v1 distributed driver use-cases (request/response bytes only unless a later task explicitly introduces remote caps).

## Consequences

### Positive

- Stable “Metal-like” SDK surface can evolve without kernel growth.
- Drivers become smaller: mostly MMIO programming + validation + reset/recovery.
- Reduces long-term complexity and makes audits feasible.

### Negative / Costs

- Requires discipline: ABI changes must go through explicit review and version bumps.
- Some early experimentation may be gated behind “non-ABI experimental” crates/APIs.

### Risks

- If we accidentally let device-specific features leak into DriverKit, the ABI grows and becomes hard to maintain.
- If we under-spec the ABI, every device grows a parallel API and we lose the benefit.

## Alternatives Considered

- **No formal ABI policy**: faster at first, but guarantees drift and fragmentation.
- **Kernel driver model**: reduces context switches but explodes TCB and makes crashes fatal.
- **Cap’n Proto-only** on-wire: may be viable later, but needs explicit OS-lite feasibility proof and dependency hygiene decision.

## Open Questions

- Do we want a separate “NexusGfx SDK ABI” that is stable on top of DriverKit, or should it be “DriverKit + gfx domain types”?
- When do we require kernel-enforced DMA isolation (IOMMU/GPU-MMU) to treat vendor blobs as untrusted?

## References

- Drivers/accelerators track: `tasks/TRACK-DRIVERS-ACCELERATORS.md`
- Device/MMIO access: `tasks/TASK-0010-device-mmio-access-model.md`
- VMO plumbing: `tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md`
- QoS/timers: `tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md`
