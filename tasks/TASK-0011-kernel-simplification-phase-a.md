---
title: TASK-0011 Kernel refactor (RFC-0001) Phase A: text-only simplification for SMP debugging window
status: Draft
owner: @kernel-team
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - RFC: docs/rfcs/RFC-0001-kernel-simplification.md
  - Kernel overview: docs/architecture/01-neuron-kernel.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

SMP bring-up is a high-debug-cost kernel change. RFC-0001 proposes logic-preserving changes that make
the kernel easier to navigate and debug (headers, invariant visibility, diagnostics index).

To reduce “kernel touch count” we treat this as the **first phase of the SMP debugging window**:
land the text-only simplification work immediately before SMP changes, with strict proofs that no
runtime behavior changed.

## Goal

Complete RFC-0001 Phase A (text-only) with **zero behavior change**, verified by the existing QEMU
marker contract.

## Scope focus (prep for TASK-0012/0013)

To minimize kernel touch count and maximize debugging ROI, this task focuses on the modules most
likely to be edited during SMP/QoS work:

- Boot + entry: `boot.rs`, `kmain.rs`
- Arch: `arch/riscv/*`
- Trap/IRQ/timer: `trap.rs`, HAL timer hooks
- Scheduler + task lifecycle: `sched/*`, `task.rs`
- Syscall surface: `syscall/*`
- Selftests/markers: `selftest/*`

Anything outside these areas is out of scope unless it is a purely mechanical header/doc fix.

## Non-Goals

- Any scheduler/boot/trap behavioral change.
- Physical reorg or subcrates.

## Constraints / invariants (hard requirements)

- **Logic-preserving only**: no code semantics changes, no symbol/ABI changes.
- **Determinism**: do not modify marker strings required by `scripts/qemu-test.sh`.
- **Kernel remains bootable**: existing marker contract stays green.

## Red flags / decision points

- **RED**:
  - None. If a change risks runtime behavior, it is out of scope for this task.
- **YELLOW**:
  - Touching many files can create merge churn. Keep commits small and mechanical.
- **GREEN**:
  - RFC-0001 explicitly scopes Phase A as text-only; ideal to land before SMP work.

## Contract sources (single source of truth)

- `docs/rfcs/RFC-0001-kernel-simplification.md`
- `docs/architecture/01-neuron-kernel.md`
- `scripts/qemu-test.sh` marker contract (must not change here)

## Stop conditions (Definition of Done)

- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` passes with **no marker list changes**.
- Docs stay in sync:
  - If any kernel-visible contracts are clarified (syscall names/IDs, scheduler invariants, acceptance marker semantics), update `docs/architecture/01-neuron-kernel.md` and the index `docs/architecture/README.md`.

## Touched paths (allowlist)

- `source/kernel/neuron/src/**`
- `docs/**` (RFC cross-links, optional indexing docs)

## Plan (small PRs)

This task is the **execution checklist** for RFC-0001 Phase A. Keep changes mechanical and reviewable.

1. **Headers (kernel-specific)**
   - Ensure the standard kernel header fields are present and accurate for the scoped modules:
     - CONTEXT, OWNERS, PUBLIC API, DEPENDS_ON, INVARIANTS, ADR
     - If present in the repo standard: STATUS/API_STABILITY/TEST_COVERAGE
   - Make invariants explicit where it helps SMP/QoS debugging:
     - W^X boundary expectations
     - “no allocation in IRQ paths”
     - determinism marker contracts

2. **Debug/diagnostics index (docs-first)**
   - Add a short “debug features index” section in a single place (either a doc or a top-level kernel module comment)
     that lists:
     - relevant feature flags (e.g. `debug_uart`, `trap_symbols`, PT verify)
     - key UART/KSELFTEST markers and what subsystem they correspond to
   - Do not change defaults or feature wiring.

3. **Test documentation uplift**
   - For kernel tests and selftests in the scoped modules:
     - add TEST_SCOPE and TEST_SCENARIOS comments where missing
     - ensure TEST_COVERAGE claims are honest (or “No tests”)
   - No test logic changes in this task.

4. **Cross-links**
   - Ensure scoped modules link to the relevant ADR/RFC for their invariants (keep links stable).

## Acceptance criteria (behavioral)

- No behavioral/ABI/marker changes.
- Kernel boots and existing QEMU marker suite stays green.
