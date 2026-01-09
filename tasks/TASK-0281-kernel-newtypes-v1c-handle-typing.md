---
title: TASK-0281 Kernel Rust idioms v1c: complete newtype coverage for all kernel handles
status: Draft
owner: @kernel-team
created: 2026-01-09
links:
  - Vision: docs/agents/VISION.md
  - Kernel overview: docs/architecture/01-neuron-kernel.md
  - Depends-on (Rust idioms baseline): tasks/TASK-0011B-kernel-rust-idioms-pre-smp.md
---

## Context

We already introduced key newtypes (`Pid`, `Asid`, `AsHandle`, `CapSlot`) to prevent handle-type confusion in syscall decoding and kernel internals. For kernel correctness and auditability (especially pre-SMP), we should complete the newtype coverage for *all* kernel handles and IDs.

## Goal

Extend the newtype pattern to cover all kernel-facing identifiers and handles (examples):

- `EndpointId`
- `IrqId`
- `HartId` / `CpuId`
- `TaskId` (if separate from `Pid`)
- `VmoHandle` / `VmoId` (depending on existing ABI model)

## Non-Goals

- Changing syscall ABI IDs or semantics (only type safety within existing contracts).
- SMP implementation itself (this is a pre-SMP type safety improvement).

## Constraints / invariants (hard requirements)

- `#[repr(transparent)]` for zero-cost wrapping where ABI-visible.
- Construction authority documented (only the owning module can mint values).
- No `unwrap/expect` in kernel paths.

## Security considerations

### Threat model

- **Handle confusion**: wrong handle type used in privileged path → capability bypass or memory corruption
- **Parsing bugs**: untrusted syscall arguments treated as internal IDs

### Security invariants (MUST hold)

- Only the owning subsystem can create/destroy handles (construction authority)
- Untrusted inputs must be validated before conversion to typed handles

### DON'T DO

- DON'T expose constructors that allow arbitrary integers to become privileged handles
- DON'T add `unsafe` unless unavoidable; if used, document invariants in-line

## Stop conditions (Definition of Done)

- All kernel handle-like integers are represented by explicit newtypes.
- Unit tests (host) validate conversion bounds for syscall decoding helpers.
- Documentation links back to construction authority rules in `01-neuron-kernel.md`.

## Touched paths (allowlist)

- `source/kernel/neuron/` (types + call sites)
- `docs/architecture/01-neuron-kernel.md` (construction authority updates as needed)
