---
title: TASK-0282 Kernel Rust idioms v1d: phantom types for capability rights (compile-time checks)
status: Draft
owner: @kernel-team
created: 2026-01-09
links:
  - Vision: docs/agents/VISION.md
  - IPC/cap model baseline: docs/rfcs/RFC-0005-kernel-ipc-capability-model.md
  - Depends-on (Rust idioms baseline): tasks/TASK-0011B-kernel-rust-idioms-pre-smp.md
  - Related: tasks/TASK-0267-kernel-ipc-v1-framed-channels-capability-transfer.md
---

## Context

Runtime rights checks are correct but easy to misapply in complex kernel paths (especially with many capability kinds). Rust can encode capability kind/rights at the type level using phantom types, reducing classes of bugs without runtime cost.

## Goal

Introduce a typed capability wrapper that encodes:

- capability kind (`Endpoint`, `Vmo`, `DeviceMmio`, etc.)
- rights (e.g. `Map`, `Send`, `Recv`)

…so that internal kernel APIs can require the correct rights at compile time.

## Non-Goals

- Replacing the external ABI contract (capabilities remain runtime objects).
- Eliminating all runtime checks (syscall boundary still validates untrusted input).

## Constraints / invariants (hard requirements)

- No new kernel dependencies.
- No `unsafe impl Send/Sync` added without explicit invariants.
- Hot paths remain allocation-free.

## Security considerations

### Threat model

- **Rights-check omission**: code path uses capability without validating rights
- **Kind confusion**: treating a `DeviceMmio` cap as a `Vmo` cap, etc.

### Security invariants (MUST hold)

- Syscall entry still enforces runtime checks (untrusted boundary)
- Typed wrappers only arise after validated lookup in `CapTable`

### DON'T DO

- DON'T leak typed wrappers into untrusted callers
- DON'T use typed wrappers to justify removing syscall-boundary checks

## Stop conditions (Definition of Done)

- A `Cap<T, R>` pattern exists for internal APIs.
- At least one kernel subsystem is refactored to use typed caps end-to-end (pilot).
- Negative tests ensure syscall boundaries still reject wrong rights/kinds.

## Touched paths (allowlist)

- `source/kernel/neuron/` (cap table wrappers + pilot subsystem)
- `docs/architecture/01-neuron-kernel.md` (capability invariants and construction authority)
