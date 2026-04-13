---
title: TASK-0284B NexusInfer v1c (host-first): ownership-based tensor/buffer states + CPU reference executor + fixture proofs
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
  - NexusInfer interop/profile binding: tasks/TASK-0280B-nexusinfer-v1b-tensor-image-interop-profile-binding.md
  - DMA buffer ownership prototype: tasks/TASK-0284-userspace-dmabuffer-ownership-v1-prototype.md
  - NexusInfer rust design: docs/architecture/nexusinfer-rust-design.md
  - NexusInfer techniques catalog: docs/architecture/nexusinfer-techniques.md
  - Zero-copy VMO plumbing: tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
---

## Context

`TASK-0284` proves that ownership/type-state style APIs are a good fit for userspace driver buffers.

`NexusInfer` needs the same discipline for tensors and model I/O:

- host-owned vs submitted/in-flight buffers,
- safe handoff and completion,
- deterministic CPU-first execution,
- and small fixture proofs that already look like later real inference flows.

This task is intentionally not “full inference runtime v1”. It is the small execution-oriented follow-up that makes the
earlier contract slices useful.

## Goal

Create a host-first `NexusInfer` CPU reference executor that models:

1. ownership-based tensor/buffer states:
   - host-owned,
   - submitted/in-flight,
   - completed/returned,
   - with typed transitions and bounded handles;
2. a minimal CPU reference executor:
   - enough to run deterministic fixture pipelines,
   - explicit submit/complete lifecycle,
   - no hidden background work;
3. a tiny useful op subset for fixture-style workloads:
   - copy/view/reshape-like metadata-safe operations,
   - layout conversion or normalization where needed for fixtures,
   - bounded reduction/select operations such as argmax/top-k if required by the fixture set;
4. deterministic host proofs:
   - output fixtures,
   - ownership/state transition tests,
   - `test_reject_*` for invalid state reuse or incompatible buffers.

## Non-Goals

- A general-purpose graph compiler.
- Full ONNX/TFLite compatibility.
- NPU/GPU execution.
- Large-model support, KV-cache tricks, or TurboQuant-style optimization work.

## Constraints / invariants (hard requirements)

- Tensor/buffer lifecycle must be encoded in the API surface, not left to comments.
- CPU may not access buffers while they are modeled as in-flight.
- Operation set must stay intentionally small and fixture-driven.
- Tests must be deterministic and host-first.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Security considerations

### Threat model

- **State misuse**: double-submit, reuse after handoff, or host access while in-flight.
- **Uninitialized or stale data exposure**: buffer contents observed across incompatible lifecycle transitions.
- **Unbounded compute**: fixture executor quietly growing into an unrestricted runtime path.

### Security invariants (MUST hold)

- Invalid lifecycle transitions are rejected by type/state API or deterministic runtime checks.
- Buffer/tensor ownership returns only through explicit completion.
- Fixture executor remains bounded in op set, sizes, and execution model.
- Rejection paths are covered by tests.

### DON'T DO

- DON'T allow host-visible mutation of in-flight buffers.
- DON'T expand the CPU reference executor into an accidental general ML framework.
- DON'T couple this task to vendor-specific kernels or accelerator assumptions.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- Deterministic tests prove:
  - valid ownership transitions compile and/or execute correctly,
  - invalid transitions are rejected,
  - CPU reference execution produces stable fixture outputs,
  - bounded op set behavior is documented and tested.

### Proof (OS/QEMU) — not required

- This task is intentionally host-first.
- Any later OS/QEMU marker should prove real pipeline behavior only, not imply hardware acceleration.

## Touched paths (allowlist)

- `userspace/` (new infer runtime/executor crate or module)
- `tests/` (host fixture and rejection tests)
- `docs/architecture/nexusinfer-rust-design.md` (only if implementation discovers a contract gap)
- `tasks/TRACK-NEXUSINFER-SDK.md`

## Plan (small PRs)

1. Define tensor/buffer type states and transitions.
2. Implement the small CPU reference executor and fixture-oriented op subset.
3. Add deterministic acceptance/rejection tests.
4. Clarify docs only if implementation reveals missing ownership/runtime language.
