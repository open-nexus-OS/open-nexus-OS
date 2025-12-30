---
title: TASK-0247 RISC-V Bring-up v1.1b (OS/QEMU): SMP (SBI HSM/IPI) + per-hart timers + virtioblkd + packagefs mount + selftests
status: Draft
owner: @kernel
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Bring-up core (host-first): tasks/TASK-0246-bringup-rv-virt-v1_1a-host-virtio-blk-image-builder-deterministic.md
  - SMP baseline: tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md
  - Bring-up v1.0: tasks/TASK-0245-bringup-rv-virt-v1_0b-os-kernel-uart-plic-timer-uartd-selftests.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need OS/QEMU integration for RISC-V Bring-up v1.1:

- SMP (up to 4 harts) with SBI HSM/IPI,
- per-hart timer ticks,
- userspace virtioblkd service,
- packagefs mount from disk image.

The prompt proposes SMP with SBI HSM/IPI and per-hart timers. `TASK-0012` already plans SMP bring-up with per-CPU runqueues and IPIs, but uses a generic approach. This task extends it with **RISC-V-specific SBI HSM/IPI** and per-hart timer programming, then adds userspace virtioblkd and packagefs mounting.

## Goal

On OS/QEMU:

1. **DTB & runner updates**:
   - update `pkg://dts/virt-nexus.dts`: `cpus` with up to 4 harts (`h0..h3`, S-mode), add `virtio_mmio@10001000` (blk) with IRQ 1x, mark `status = "okay"`
   - rebuild `virt-nexus.dtb`
   - runner/QEMU: allow `SMP=${SMP:-4}`, default 4; add virtio-blk disk image: `-drive if=none,id=pkgfs,format=raw,file=build/pkgfs.img -device virtio-blk-device,drive=pkgfs`
2. **Kernel (arch/riscv): HSM/IPI + per-hart timer ticks**:
   - **HSM/IPI SBI shims**: `sbi::hsm::hart_start(hid, entry, opaque)`, `hart_stop()`, `send_ipi(hartmask)`
   - **Secondary bring-up**: boot hart0 parses DTB, sets shared boot state, then starts harts [1..N-1]; each secondary sets `stvec`, `sie`, joins scheduler idle loop, prints marker
   - **Per-hart timer**: program `sbi::time::set_timer(now + tick_ns)` per hart; on timer interrupt increment per-CPU tick counter and reschedule
   - **IRQ/IPI dispatch**: handle SSIP (software IPI) to trigger reschedule; minimal handler with marker
   - **Scheduler hints**: expose `yield_to(any_hart)` stub; keep policy simple (round-robin/CFS-like placeholder), ensure thread-safe runqueue ops
   - markers: `neuron: hart0 boot virt smp=4`, `neuron: hartX online`, `neuron: hartX tick n=…`, `neuron: ipi hartX->hartY`
3. **Userspace virtioblkd service** (`source/services/virtioblkd/`):
   - implement virtio-mmio blk frontend (read-only) using library from `TASK-0246`
   - probe virtio-mmio region from DT (base/irq), read identify config, enforce RO
   - API (`virtio_blk.capnp`): `read(lba, count)`, `info()` (sectors, sectorSize)
   - marker: `virtioblkd: ready`, `virtioblkd: mmio=0x… irq=… sectors=…`
4. **Packagefs block-backed mount**:
   - add block-backed read-only view: implement thin sector cache over `Vblk.read()` and adapt existing `packagefs` parser to read from image `pkgfs.img`
   - on `nexus-init`, start `virtioblkd` then mount `packagefs` at `pkg://` using block device; fallback to in-memory fixtures if device missing
   - marker: `pkgfs: mounted from virtio-blk ro`
5. **Diagnostics CLI** (`tools/nx-hw/`):
   - `nx hw smp` → lists online harts and per-hart tick counters
   - `nx hw vblk info` → prints sector count/size and first bytes checksum
   - markers: `nx: smp online=4`, `nx: vblk sectors=…`
6. **OS selftests + postflight**.

## Non-Goals

- Full SMP scheduler (extends `TASK-0012` with RISC-V-specific features only).
- Write support for virtio-blk (read-only for packagefs only).
- Real hardware (QEMU `virt` only).

## Constraints / invariants (hard requirements)

- **No duplicate SMP authority**: This task extends `TASK-0012` with RISC-V-specific SBI HSM/IPI. Do not create a parallel SMP implementation.
- **No duplicate virtio-blk authority**: `virtioblkd` uses the library from `TASK-0246`. `TASK-0009` will use the same library for statefs (read-write).
- **Determinism**: SMP bring-up, per-hart timers, and virtio-blk operations must be stable given the same inputs.
- **Bounded resources**: SMP is limited to 4 harts; virtio-blk reads are bounded (128 KiB per call).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (SMP authority drift)**:
  - Do not create a parallel SMP implementation. Extend `TASK-0012` with RISC-V-specific SBI HSM/IPI and per-hart timer programming.
- **YELLOW (SBI HSM availability)**:
  - SBI HSM (Hart State Management) must be available in OpenSBI. If not, this task must document fallback or gate on OpenSBI version.

## Contract sources (single source of truth)

- QEMU marker contract: `scripts/qemu-test.sh`
- Bring-up core: `TASK-0246`
- SMP baseline: `TASK-0012` (per-CPU runqueues, IPIs)
- Bring-up v1.0: `TASK-0245` (DTB, PLIC, timer)

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — gated

UART markers:

- `neuron: hart0 boot virt smp=4`
- `neuron: hartX online` (for all configured harts)
- `neuron: hartX tick n=…`
- `neuron: ipi hartX->hartY`
- `virtioblkd: ready`
- `virtioblkd: mmio=0x… irq=… sectors=…`
- `pkgfs: mounted from virtio-blk ro`
- `SELFTEST: smp per-hart ticks ok`
- `SELFTEST: smp ipi ok`
- `SELFTEST: pkgfs virtio ro ok`

## Touched paths (allowlist)

- `pkg://dts/virt-nexus.dts` (extend: SMP harts, virtio-blk device)
- `source/kernel/neuron/src/arch/riscv/` (extend: SBI HSM/IPI shims, secondary bring-up, per-hart timer)
- `source/services/virtioblkd/` (new)
- `source/services/packagefsd/` (extend: block-backed mount)
- `source/init/nexus-init/` (extend: start virtioblkd + mount packagefs)
- `scripts/run-qemu-rv64.sh` (extend: SMP parameter, virtio-blk disk image)
- `tools/nx-hw/` (extend: smp & virtio-blk diagnostics)
- `source/apps/selftest-client/` (markers)
- `docs/bringup/virt_smp_v1_1.md` (new)
- `docs/storage/virtio_blk.md` (new)
- `tools/postflight-bringup-rv_virt-v1_1.sh` (new)

## Plan (small PRs)

1. **DTB & runner updates**
   - DTB: SMP harts + virtio-blk device
   - runner: SMP parameter + virtio-blk disk image

2. **Kernel SMP (SBI HSM/IPI + per-hart timers)**
   - SBI HSM/IPI shims
   - secondary bring-up
   - per-hart timer programming
   - IRQ/IPI dispatch
   - markers

3. **Userspace virtioblkd + packagefs mount**
   - virtioblkd service
   - packagefs block-backed mount
   - nexus-init wiring
   - markers

4. **Diagnostics + selftests**
   - nx-hw CLI extensions
   - OS selftests + postflight

## Acceptance criteria (behavioral)

- SMP online with per-hart timer ticks; IPIs functional.
- `virtioblkd` probes and serves read-only.
- `packagefs` mounts from `pkgfs.img`; known file reads successfully.
- All three OS selftest markers are emitted.
