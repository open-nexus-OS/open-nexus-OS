---
title: TASK-0011 Kernel simplification (RFC-0001): Phase A (text-only) + Phase B (physical reorg)
status: In Review
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

Kernel work is high-debug-cost. RFC-0001 proposes logic-preserving changes that make the kernel
easier to navigate and debug (headers, invariant visibility, diagnostics index).

We also want the kernel to have a stable physical layout. The longer we postpone a directory
reorganization, the more painful it becomes (merge churn, higher rename surface, more stale links).
This task therefore includes an explicitly scoped physical reorganization phase with strict proof
gates that no runtime behavior changed.

## Goal

Complete RFC-0001 simplification work in two phases with **zero behavior change**, verified by the
existing QEMU marker contract.

## Phases (explicit; no ambiguity)

### Phase A: Text-only simplification (documentation + headers)

- Add/normalize kernel module headers.
- Add a single debug/diagnostics index (docs-first).
- Add TEST_SCOPE / TEST_SCENARIOS documentation where missing.
- Add/verify cross-links (ADR/RFC/architecture docs).
- No file moves in this phase.

### Phase B: Physical directory reorganization (logic-preserving)

- Physically reorganize `source/kernel/neuron/src/` into a stable directory structure that matches
  the responsibility taxonomy.
- Only do mechanical moves/renames and module wiring updates. No semantic refactors.
- Update `docs/**` to match the new paths.

## Scope focus (what this task touches)

To minimize kernel touch count and maximize debugging ROI, this task focuses on the modules that
are commonly used as navigation anchors during kernel debugging:

- Boot + entry: `boot.rs`, `kmain.rs`
- Arch: `arch/riscv/*`
- Trap/IRQ/timer: `trap.rs`, HAL timer hooks
- Scheduler + task lifecycle: `sched/*`, `task.rs`
- Syscall surface: `syscall/*`
- Selftests/markers: `selftest/*`

Anything outside these areas is out of scope unless it is a purely mechanical header/doc fix.

## Non-Goals

- Any scheduler/boot/trap behavioral change.
- Any change to the syscall ABI (numbers, error semantics, struct layouts).
- Any marker string changes required by `scripts/qemu-test.sh`.
- Subcrate split.
- Any default/semantic changes to feature wiring.

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
  - The work is bounded, mechanical, and proven via the existing QEMU marker contract.

## Security considerations

### Threat model
- N/A (text-only documentation changes, no runtime behavior modifications)

### Security invariants (MUST hold)
All existing kernel security invariants remain unchanged and must be explicitly documented in module headers:
- **W^X enforcement**: Writable and executable mappings are mutually exclusive (enforced at `SYS_AS_MAP` boundary)
- **Capability rights**: Rights can only be restricted, never escalated (enforced in `cap_transfer`)
- **User/Kernel boundary**: No ambient authority; all access requires explicit capabilities
- **MMIO mappings**: Device memory is USER|RW only, never EXEC (enforced in `mmio_map`)
- **ASID isolation**: Address spaces are isolated via ASID; no cross-AS access without explicit mapping
- **Bootstrap integrity**: Child tasks receive only explicitly granted capabilities via `BootstrapMsg`

### DON'T DO (explicit prohibitions)
- DON'T modify any security-critical code paths (W^X checks, capability validation, MMIO mapping logic)
- DON'T change UART marker strings that prove security enforcement (e.g., `KSELFTEST: w^x enforced`)
- DON'T alter capability transfer semantics or rights masking logic
- DON'T touch syscall error handling that returns `-EPERM` for security violations
- DON'T modify address space isolation logic (ASID allocation, SATP switching)
- DON'T change the `BootstrapMsg` layout or capability seeding logic

### Attack surface impact
- None (documentation-only changes do not modify attack surface)

### Mitigations
- N/A (no new code paths introduced)

### Documentation requirements (security-specific)
When adding headers to security-critical modules, explicitly document:
1. **For `mm/` (Memory Management)**:
   - W^X enforcement points
   - ASID isolation guarantees
   - Guard page placement strategy
2. **For `cap/` (Capabilities)**:
   - Rights intersection rules
   - Capability derivation constraints
   - Slot allocation limits
3. **For `syscall/` (Syscall handlers)**:
   - Which syscalls enforce W^X (`as_map`, `mmio_map`)
   - Which syscalls check capabilities (`send`, `recv`, `map`)
   - Error codes for security violations (`-EPERM`, `-EINVAL`)
4. **For `trap.rs` (Trap handling)**:
   - User/Kernel mode transitions
   - Privilege escalation prevention
   - Trap handler isolation

## Contract sources (single source of truth)

- `docs/rfcs/RFC-0001-kernel-simplification.md`
- `docs/architecture/01-neuron-kernel.md`
- `scripts/qemu-test.sh` marker contract (must not change here)

## Stop conditions (Definition of Done)

### Phase A stop conditions

- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` passes with **no marker list changes**.
- Docs stay in sync (paths unchanged in Phase A; content may be clarified).

### Phase B stop conditions

- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` passes with **no marker list changes**.
- `cargo test --workspace` passes.
- No semantic code changes were introduced (moves/renames/wiring only).
- Docs updated so kernel path references resolve.

## Touched paths (allowlist)

- `source/kernel/neuron/src/**`
- `docs/**` (RFC cross-links, optional indexing docs)

## Plan (small PRs)

This task is the **execution checklist** for RFC-0001. Keep changes mechanical and reviewable.

### Phase A plan (text-only)

1. **Headers (kernel-specific)**
   - Ensure the standard kernel header fields are present and accurate for the scoped modules:
     - CONTEXT, OWNERS, PUBLIC API, DEPENDS_ON, INVARIANTS, ADR
     - If present in the repo standard: STATUS/API_STABILITY/TEST_COVERAGE
   - Make invariants explicit where it helps debugging:
     - W^X boundary expectations
     - "no allocation in IRQ paths"
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

### Phase B plan (physical reorg; wiring-only)

#### Target directory tree (normative)

This phase reorganizes the kernel into a stable tree. The goal is to keep `arch/` and `hal/`
separate, keep `core/` intentionally small, and avoid a "utils junk drawer" by using `diag/` for
cross-cutting debug/determinism helpers.

Target tree:

```text
source/kernel/neuron/src/
  arch/
    riscv/
  cap/
  core/
  diag/
    sync/
  hal/
  ipc/
  mm/
  sched/
  selftest/
  syscall/
  task/
  lib.rs
  types.rs
```

#### Move map (explicit)

Move the following files/directories to the new locations:

- `boot.rs` -> `core/boot.rs`
- `kmain.rs` -> `core/kmain.rs`
- `trap.rs` -> `core/trap.rs`
- `panic.rs` -> `core/panic.rs`
- `satp.rs` -> `mm/satp.rs`
- `task.rs` -> `task/mod.rs`
- `bootstrap.rs` -> `task/bootstrap.rs`
- `log.rs` -> `diag/log.rs`
- `uart.rs` -> `diag/uart.rs`
- `determinism.rs` -> `diag/determinism.rs`
- `liveness.rs` -> `diag/liveness.rs`
- `sync/mod.rs` -> `diag/sync/mod.rs`
- `sync/dbg_mutex.rs` -> `diag/sync/dbg_mutex.rs`

Keep these directories as-is (they already match the taxonomy):

- `arch/`
- `cap/`
- `hal/`
- `ipc/`
- `mm/` (except `satp.rs`, which moves into it)
- `sched/`
- `selftest/`
- `syscall/`

#### Phase B prohibitions

- Do not change logic, formatting-only refactors, or any runtime behavior.
- Do not change marker strings.
- Do not introduce new public surface area or change visibility policies.
- Do not add/remove dependencies.

## Acceptance criteria (behavioral)

- No behavioral/ABI/marker changes.
- Kernel boots and existing QEMU marker suite stays green.
