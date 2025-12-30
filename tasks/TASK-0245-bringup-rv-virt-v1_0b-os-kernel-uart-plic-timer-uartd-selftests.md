---
title: TASK-0245 Hardware Bring-up (RISC-V virt) v1.0b (OS/QEMU): kernel UART/PLIC/timer + userspace uartd + selftests
status: Draft
owner: @kernel
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Bring-up core (host-first): tasks/TASK-0244-bringup-rv-virt-v1_0a-host-dtb-sbi-shim-deterministic.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need OS/QEMU kernel integration for Hardware Bring-up v1.0:

- kernel early UART printk (extends existing UART support),
- PLIC initialization,
- timer tick via SBI,
- userspace `uartd` service,
- QEMU wiring with DTB handoff.

The prompt proposes kernel UART/PLIC/timer and userspace uartd. Existing kernel already has UART early printk (`source/kernel/neuron/src/uart.rs`). This task extends it with DTB-based discovery and adds PLIC/timer initialization, then hands off to userspace `uartd`.

## Goal

On OS/QEMU:

1. **Kernel arch bring-up** (`source/kernel/neuron/src/arch/riscv/`):
   - **Early UART (polled)**: extend existing UART writer to use base from DTB (fallback `0x10000000`)
   - **DTB parse**: small reader to fetch timebase, UART, PLIC base, and hart count; log parsed values via early printk
   - **PLIC minimal init**: set threshold 0, enable UART IRQ for hart 0; leave others masked; external IRQs get acknowledged but forwarded later to userspace (stub)
   - **Timer tick**: init timer tick (e.g., 1kHz via SBI set_timer); rearm on interrupt
   - **Trap/interrupt**: ensure `stvec` → existing `trap.S`; enable `sie` bits (SSIE/STIE/SEIE); provide `irq_dispatch()` that ticks timer and posts a marker
   - **Monotonic time**: `nsec()` from SBI get_time() with DTB timebase; stable conversion
   - **Boot flow**: `kmain` → early printk banner "NEURON (virt)", parse DTB, init timer tick, init PLIC, then spawn `nexus-init`
   - markers: `neuron: boot virt dtb-ok tb=10000000`, `neuron: uart early printk ok`, `neuron: plic enabled`, `neuron: timer tick start`
2. **Userspace UART driver** (`source/services/uartd/`):
   - MMIO NS16550 driver in userspace (mapped via devmem cap or HAL mapping API) using base from DTB parsed by a tiny **devtree proxy** service (or pass via boot args env)
   - API (`uart.capnp`): `write`, `read` (polled for now), `attachConsole` (binds to logd sink)
   - on start, prints `uartd: userspace console ready` and subscribes to `logd` to flush logs to UART
   - kernel stops using early printk except for panics
   - markers: `uartd: ready`, `uartd: mmio=0x10000000 irq=10`, `uartd: attach console ok`
3. **nexus-init extension**:
   - start `logd` then `uartd.attachConsole()`
   - print "HELLO FROM USERSPACE" via log pipeline
   - marker: `userspace: hello`
4. **QEMU wiring**:
   - update QEMU launch to pass DTB and use OpenSBI:
     - machine `virt`, `-bios default` (OpenSBI), `-machine virt`, `-nographic`, `-smp 1`, `-m 1024`
     - append kernel arguments to expose DTB pointer if needed
   - keep RUN_TIMEOUT and uart.log rotation from soak work
5. **Diagnostics CLI** (`tools/nx-hw/`):
   - `nx hw dt` (dump parsed DT nodes: uart, plic, timebase)
   - `nx hw sbi` (print sbi base/time info)
   - `nx hw irq` (show plic enable/threshold state)
   - markers: `nx: hw dt tb=10000000`, `nx: hw sbi time=…`
6. **OS selftests + postflight**.

## Non-Goals

- Full PLIC support (minimal init only).
- Full DTB spec compliance (minimal subset only).
- Real hardware (QEMU `virt` only).

## Constraints / invariants (hard requirements)

- **No duplicate UART authority**: kernel early printk is for boot/panic only; userspace `uartd` is the canonical console after init. Do not create parallel UART drivers.
- **Determinism**: DTB parsing, PLIC init, and timer tick must be stable given the same inputs.
- **Bounded resources**: DTB parsing is size-bounded; PLIC init is minimal.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (UART authority drift)**:
  - Do not create a parallel userspace UART driver that conflicts with kernel early printk. Kernel uses early printk for boot/panic only; `uartd` takes over after init.
- **YELLOW (DTB handoff)**:
  - DTB pointer must be passed from OpenSBI to kernel. Document the handoff mechanism explicitly (boot args, register, or memory location).

## Contract sources (single source of truth)

- QEMU marker contract: `scripts/qemu-test.sh`
- Bring-up core: `TASK-0244`
- Existing UART: `source/kernel/neuron/src/uart.rs`

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — gated

UART markers:

- `neuron: boot virt dtb-ok tb=10000000`
- `neuron: uart early printk ok`
- `neuron: plic enabled`
- `neuron: timer tick start`
- `uartd: ready`
- `uartd: mmio=0x10000000 irq=10`
- `uartd: attach console ok`
- `userspace: hello`
- `SELFTEST: bringup uart early+userspace ok`
- `SELFTEST: bringup plic+timer ok`

## Touched paths (allowlist)

- `source/kernel/neuron/src/arch/riscv/` (extend: DTB parse, PLIC init, timer tick)
- `source/kernel/neuron/src/uart.rs` (extend: DTB-based base address)
- `source/services/uartd/` (new)
- `source/init/nexus-init/` (extend: start logd + uartd)
- `scripts/run-qemu-rv64.sh` (extend: DTB handoff)
- `tools/nx-hw/` (new)
- `source/apps/selftest-client/` (markers)
- `docs/bringup/virt.md` (new)
- `docs/bringup/troubleshoot.md` (new)
- `tools/postflight-bringup-rv_virt-v1_0.sh` (new)

## Plan (small PRs)

1. **Kernel arch bring-up**
   - DTB parse in kernel
   - PLIC init
   - timer tick via SBI
   - extend early UART with DTB base
   - markers

2. **Userspace uartd**
   - MMIO NS16550 driver
   - logd integration
   - nexus-init wiring
   - markers

3. **QEMU wiring + diagnostics + selftests**
   - DTB handoff in QEMU launch
   - nx-hw CLI
   - OS selftests + postflight

## Acceptance criteria (behavioral)

- Kernel boot prints early over UART, parses DTB, enables timer & PLIC.
- Userspace `uartd` takes over console; "HELLO FROM USERSPACE" visible.
- All OS selftest markers are emitted.
