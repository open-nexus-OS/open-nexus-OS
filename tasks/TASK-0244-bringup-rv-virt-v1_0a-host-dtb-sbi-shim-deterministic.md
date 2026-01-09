---
title: TASK-0244 Hardware Bring-up (RISC-V virt) v1.0a (host-first): DTB parser + SBI shim + deterministic tests
status: Draft
owner: @kernel
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need hardware bring-up for RISC-V `virt` machine:

- DTB (Device Tree Blob) parsing to discover base addresses/timebase,
- SBI (Supervisor Binary Interface) shim for time/timer operations,
- deterministic host tests.

The prompt proposes DTB skeleton and SBI shim. This task delivers the **host-first core** (DTB parser, SBI shim, tests) before OS/QEMU kernel integration.

## Goal

Deliver on host:

1. **DTB parser library** (`userspace/libs/dtb/` or `source/kernel/neuron/src/arch/riscv/dtb/`):
   - parse DTB to extract:
     - UART base address (default `0x10000000`, fallback if not in DTB)
     - PLIC base address (default `0x0c000000`)
     - timebase frequency (default `10000000`)
     - hart count
   - deterministic parsing (no host-specific behavior)
   - error handling for malformed DTB
2. **SBI shim library** (`source/kernel/neuron/src/arch/riscv/sbi/`):
   - `sbi::time::set_timer(u64)` (delegates to SBI ecall)
   - `sbi::time::get_time()` (delegates to SBI ecall, returns monotonic ns)
   - `sbi::base::impl_id()` (returns OpenSBI implementation ID)
   - deterministic error paths
3. **Monotonic time conversion**:
   - `nsec()` from SBI get_time() with DTB timebase
   - stable conversion (no host locale leakage)
4. **DTB skeleton** (`pkg://dts/virt-nexus.dts`):
   - minimal, deterministic DTS describing:
     - `cpus` (1 hart initially)
     - `memory@80000000`
     - `uart@10000000` (ns16550a, irq 10)
     - `plic@0c000000` (irq domain)
     - `virtio-mmio@10001000` (present but disabled for now)
     - `/timebase-frequency = <10000000>;`
   - build step: compile to `pkg://dts/virt-nexus.dtb` in image
5. **Host tests** proving:
   - DTB parser extracts expected values (UART/PLIC bases, timebase, hart count)
   - SBI shim returns deterministic values (mocked calls)
   - `nsec()` conversion correct

## Non-Goals

- OS/QEMU kernel integration (deferred to v1.0b).
- Full DTB spec compliance (minimal subset only).
- Real hardware (QEMU `virt` only).

## Constraints / invariants (hard requirements)

- **Determinism**: DTB parsing, SBI calls, and time conversion must be stable given the same inputs.
- **Bounded resources**: DTB parsing is size-bounded; SBI calls are deterministic.
- **no_std compatibility**: DTB parser and SBI shim must work in no_std kernel context.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **YELLOW (DTB location)**:
  - DTB pointer must be passed from bootloader/OpenSBI to kernel. Document the handoff mechanism explicitly.

## Security considerations

### Threat model

- **Malformed DTB**: crafted DTB causing parser out-of-bounds reads or unbounded work
- **SBI shim misuse**: incorrect error handling or unsafe argument handling causing undefined behavior
- **Information leakage**: logs accidentally exposing host-specific or non-deterministic values

### Security invariants (MUST hold)

- **Bounded parsing**: DTB parser is size-bounded and validates offsets/lengths before reading
- **No panics on untrusted input**: malformed DTB yields deterministic errors (no `unwrap/expect`)
- **Deterministic behavior**: host tests and parsing results are stable given the same DTB input

### DON'T DO (explicit prohibitions)

- DON'T parse DTB with unbounded recursion or unbounded allocation
- DON'T treat DTB fields as trusted without bounds checks
- DON'T use wall-clock or host locale data in conversions/tests

## Contract sources (single source of truth)

- Testing contract: `scripts/qemu-test.sh`
- RISC-V virt machine spec (QEMU)

## Stop conditions (Definition of Done)

### Proof (Host) — required

`cargo test -p bringup_rv_virt_v1_host` green (new):

- DTB parse: feed fixture DTB; expect UART/PLIC bases and timebase parsed as constants
- SBI shim: mock calls return deterministic values; `nsec()` conversion correct

## Touched paths (allowlist)

- `source/kernel/neuron/src/arch/riscv/dtb/` (new; DTB parser)
- `source/kernel/neuron/src/arch/riscv/sbi/` (new; SBI shim)
- `pkg://dts/virt-nexus.dts` (new; DTB skeleton)
- `pkg://dts/virt-nexus.dtb` (new; compiled DTB)
- `tests/bringup_rv_virt_v1_host/` (new)
- `docs/bringup/virt.md` (new, host-first sections)

## Plan (small PRs)

1. **DTB parser + skeleton**
   - DTB parser library (minimal subset)
   - DTB skeleton (virt-nexus.dts)
   - build step (compile DTS → DTB)
   - host tests

2. **SBI shim + time conversion**
   - SBI shim library (time/base calls)
   - monotonic time conversion
   - host tests

3. **Docs**
   - host-first docs

## Acceptance criteria (behavioral)

- DTB parser extracts expected values correctly.
- SBI shim returns deterministic values.
- `nsec()` conversion is correct.
