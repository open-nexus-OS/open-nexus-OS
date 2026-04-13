---
title: TASK-0280B NexusInfer v1b (host-first): tensor/image interop + profile binding + bounded submit semantics
status: Draft
owner: @runtime @ui
created: 2026-04-10
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - NexusInfer track: tasks/TRACK-NEXUSINFER-SDK.md
  - DriverKit core contracts: tasks/TASK-0280-driverkit-v1-core-contracts-queues-fences-buffers.md
  - DMA buffer ownership prototype: tasks/TASK-0284-userspace-dmabuffer-ownership-v1-prototype.md
  - Zero-copy VMO plumbing: tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - QoS/timers contract: tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md
  - Gfx resource model: docs/architecture/nexusgfx-resource-model.md
  - Gfx sync/lifetime model: docs/architecture/nexusgfx-sync-and-lifetime.md
  - NexusInfer runtime profiles: docs/architecture/nexusinfer-runtime-profiles.md
  - NexusInfer rust design: docs/architecture/nexusinfer-rust-design.md
---

## Context

`TRACK-NEXUSINFER-SDK` already fixes the architectural direction: metadata on the control plane, bulk tensor/model/image
payloads on VMO/filebuffer-like resources, CPU-first bring-up, and shared resource/sync posture with `NexusGfx`.

The next drift risk is not model execution itself, but interop:

- image/frame import/export,
- tensor metadata and layout rules,
- runtime profile binding,
- and bounded submit/completion semantics.

If these are left vague, media, games, and editors will each invent their own tensor/image paths before the runtime
contract exists.

## Goal

Deliver the first concrete `NexusInfer` contract slice that defines:

1. tensor descriptors:
   - shape, dtype, layout, stride/offset rules,
   - bounded metadata validation,
   - explicit import/export posture;
2. image/tensor interop:
   - image-backed resources can be imported/exported using the same zero-copy posture as `NexusGfx`,
   - no infer-only image universe,
   - explicit compatibility/rejection rules;
3. profile binding:
   - an inference session binds to an explicit runtime profile,
   - profile fields relevant to executor/precision/residency/deadline are validated and surfaced deterministically;
4. bounded submit/completion:
   - minimal submit path with deadlines,
   - completion state/error mapping suitable for CPU-first execution and later NPU executors.

## Non-Goals

- Real NPU hardware integration.
- Large model execution or a full graph runtime.
- App-specific model-loading UX.
- Search v2.1 or product-specific semantic features.

## Constraints / invariants (hard requirements)

- Large payloads remain on VMO/filebuffer-like resources; metadata only on the control plane.
- All shapes/layouts/strides/formats are validated before submit.
- Session/profile binding is explicit; no hidden auto-profile switching.
- Image/tensor interop must remain compatible with shared `NexusGfx` resource/sync vocabulary.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Security considerations

### Threat model

- **Oversized tensor metadata**: shape/stride/layout abuse causing overflow or denial of service.
- **Format confusion**: invalid image/tensor reinterpretation leaking or corrupting data.
- **Profile abuse**: requesting unsupported residency/precision/deadline combinations and bypassing safety checks.

### Security invariants (MUST hold)

- Tensor/image metadata is bounded and validated before submit.
- Unsupported interop combinations fail deterministically.
- Profiles are explicit, validated, and deny-by-default outside supported combinations.
- No tensor/model/image bytes are embedded inline in the control plane once bulk handles are available.

### DON'T DO

- DON'T create infer-only resource/fence semantics separate from `NexusGfx`/DriverKit posture.
- DON'T accept unbounded shape rank, element count, or stride values.
- DON'T silently coerce unsupported profile combinations.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- Deterministic tests prove:
  - valid tensor descriptors are accepted,
  - invalid ranks/shapes/strides/layouts are rejected,
  - image/tensor import/export compatibility rules are enforced,
  - profile binding accepts supported combinations and rejects unsupported ones,
  - bounded submit/completion semantics are stable.

### Proof (OS/QEMU) — optional/gated

- If an OS-facing infer service exists later, acceptable markers include:
  - `infer: profile bind ok`
  - `infer: submit ok`
  - `SELFTEST: infer tensor interop ok`

- No marker may imply NPU acceleration unless a real NPU executor exists.

## Touched paths (allowlist)

- `userspace/` (new infer core crate or runtime module)
- `source/services/` (only if a minimal infer service scaffold already exists)
- `docs/architecture/nexusinfer-runtime-profiles.md` (only if contract gaps are discovered)
- `docs/architecture/nexusinfer-rust-design.md` (only if contract gaps are discovered)
- `tasks/TRACK-NEXUSINFER-SDK.md`

## Plan (small PRs)

1. Define tensor/image descriptors and bounded validation rules.
2. Add explicit session/profile binding with deterministic error surfaces.
3. Add a minimal submit/completion contract compatible with CPU-first execution.
4. Add host tests for acceptance/rejection and interop behavior.
