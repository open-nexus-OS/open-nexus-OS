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
- **No duplicate virtio-blk authority**: exactly one blk authority may own the virtio-blk device in a given boot profile.
  `virtioblkd` (this task) uses the library from `TASK-0246`. The persistence path from `TASK-0009` already consumes
  virtio-blk for statefs (read-write) in the current OS profile; if `virtioblkd` is introduced, document which profile
  uses which authority and keep the ownership exclusive.
- **Determinism**: SMP bring-up, per-hart timers, and virtio-blk operations must be stable given the same inputs.
- **Bounded resources**: SMP is limited to 4 harts; virtio-blk reads are bounded (128 KiB per call).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (SMP authority drift)**:
  - Do not create a parallel SMP implementation. Extend `TASK-0012` with RISC-V-specific SBI HSM/IPI and per-hart timer programming.
- **YELLOW (SBI HSM availability)**:
  - SBI HSM (Hart State Management) must be available in OpenSBI. If not, this task must document fallback or gate on OpenSBI version.

## Security considerations

### Threat model
- **Hart boot hijacking**: Malicious code attempting to start harts with arbitrary entry points
- **SBI HSM abuse**: Unauthorized hart start/stop operations causing DoS
- **IPI flooding**: Malicious tasks triggering excessive IPIs to cause DoS
- **Per-hart timer manipulation**: Tasks attempting to manipulate other harts' timers
- **Virtio-blk attacks**: Malicious disk images causing buffer overflows or information leakage
- **Packagefs tampering**: Modified disk images bypassing signature verification

### Security invariants (MUST hold)

All existing SMP security invariants from TASK-0012 and TASK-0277 remain unchanged, plus:
- **Hart boot authentication**: Only kernel can start secondary harts (SBI HSM is privileged)
- **Entry point validation**: Hart entry points are validated (must be kernel code, not user-controllable)
- **IPI sender validation**: IPI sender is hardware hart ID (unforgeable)
- **Per-hart timer isolation**: Each hart's timer is isolated (no cross-hart timer manipulation)
- **Virtio-blk read-only**: Virtio-blk device is read-only (no writes allowed)
- **Packagefs integrity**: Packagefs mount verifies signatures
  - **Phase 1 (this task)**: Use trusted build artifact (QEMU virt bring-up only)
  - **Phase 2 (production)**: See `docs/security/signing-and-policy.md` for bundle signing

### DON'T DO (explicit prohibitions)

- DON'T allow user code to call SBI HSM functions (kernel-only)
- DON'T trust user-provided hart entry points (validate against kernel code range)
- DON'T allow unbounded IPI sending (enforce rate limits)
- DON'T allow cross-hart timer manipulation (each hart owns its timer)
- DON'T enable virtio-blk writes in this task (read-only for packagefs)
- DON'T mount packagefs without signature verification (unless trusted build)
- DON'T log hart IDs or timer values in production (information leakage)

### Attack surface impact

- **Minimal**: SBI HSM is privileged (only kernel can call)
- **Controlled**: Virtio-blk is read-only (no write attacks)
- **Bounded**: IPI rate limits prevent flooding

### Mitigations

- **SBI HSM privilege**: Only kernel S-mode can call SBI HSM (hardware-enforced)
- **Entry point validation**: Hart entry points validated against kernel code range
- **IPI rate limiting**: Kernel enforces max IPIs per hart per second (e.g., 1000/sec)
- **Per-hart timer ownership**: Each hart owns its timer (no cross-hart access)
- **Virtio-blk read-only**: Device configured as read-only (no write capability)
- **Packagefs signature verification**: Mount verifies signatures (or uses trusted build artifact)
- **Audit logging**: Hart start/stop, IPI, and virtio-blk operations logged

### RISC-V SMP security requirements

When implementing RISC-V SMP features, ensure:
1. **SBI HSM validation**: Validate hart IDs and entry points before calling SBI HSM
2. **IPI authentication**: Verify IPI sender is valid hart ID (hardware-enforced)
3. **Timer isolation**: Each hart's timer is isolated (no shared timer state)
4. **Virtio-blk bounds**: Validate LBA ranges and sector counts (prevent out-of-bounds reads)
5. **Packagefs integrity**: Verify packagefs signatures before mounting (or use trusted build)

### Virtio-blk security policy

**Read-only enforcement**:
- Device configured as read-only at initialization
- Write requests rejected with error (not silently ignored)
- Audit logging for write attempts (security violation)

**Bounds validation**:
- LBA (Logical Block Address) validated against device size
- Sector count validated (max 256 sectors per request)
- Buffer size validated (prevent overflows)

**Error handling**:
- Device errors logged but do not crash kernel
- Malformed responses rejected (bounds checks on all fields)
- Timeout on slow responses (prevent DoS)

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

### Docs gate (keep architecture entrypoints in sync)

- If bring-up sequencing, storage mount semantics, or marker ownership changes, update:
  - `docs/architecture/06-boot-and-bringup.md` (boot/bring-up pipeline)
  - `docs/architecture/09-nexus-init.md` (bring-up orchestration)
  - `docs/architecture/12-storage-vfs-packagefs.md` (packagefs/vfs responsibilities and proofs)
  - and the index `docs/architecture/README.md`

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
