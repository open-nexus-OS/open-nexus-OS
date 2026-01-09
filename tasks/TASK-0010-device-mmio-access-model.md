---
title: TASK-0010 Device access model v1: safe userspace MMIO for virtio devices (enables virtio-net/virtio-blk)
status: In Progress
owner: @kernel-team @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - RFC: docs/rfcs/RFC-0005-kernel-ipc-capability-model.md
  - Unblocks: tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - Unblocks: tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Unblocks: tasks/TRACK-DRIVERS-ACCELERATORS.md (GPU/NPU/VPU/Audio/Camera/ISP userspace drivers)
  - Unblocks: tasks/TRACK-NETWORKING-DRIVERS.md (virtio-net userspace frontend)
---

## Context

Our vision is “kernel minimal, drivers in userspace”. On QEMU `virt`, virtio devices are MMIO.
Today userspace can map VMOs (`as_map`) but cannot map arbitrary physical MMIO ranges.

That makes true userspace virtio frontends (net/blk) **impossible** unless the kernel provides a safe,
capability-gated way to expose device MMIO to a specific userspace driver/service.

Track alignment: this is a foundational prerequisite for both `tasks/TRACK-DRIVERS-ACCELERATORS.md` and
`tasks/TRACK-NETWORKING-DRIVERS.md` (userspace device-class services require a safe MMIO/IRQ/DMA contract).

## Goal

Provide the minimal kernel/userspace contract to allow a userspace service to:

- receive a capability representing a specific device MMIO range (virtio-net/virtio-blk),
- map it into its address space read/write (never executable),
- and use it to drive a virtio queue implementation in userspace.

## Non-Goals

- A full device manager and dynamic enumeration framework.
- Exposing arbitrary physical memory to userspace.
- Interrupt routing (polling-only is acceptable for MVP).

## Constraints / invariants (hard requirements)

- **Security floor**:
  - mapping must be capability-gated (no ambient MMIO),
  - mappings must be **USER + RW**, never executable,
  - mapping range must be fixed and bounded to the device BAR/MMIO window.
- **Kernel minimal**: provide a tiny primitive; policy and driver logic remain in userspace.
- **Determinism**: mapping errors deterministic; no “success” logs without real capability.

## Red flags / decision points

- **RED (blocking / must decide now)**:
  - This task **requires kernel work**. If "kernel untouched" is absolute, then userspace virtio drivers
    must be deferred or replaced with a different backend (e.g., host-provided VMO block service) and the
    vision "userspace drivers" is not achievable on QEMU `virt`.
- **YELLOW (risky / likely drift / needs follow-up)**:
  - Device enumeration: we can start with a fixed, build-time wired device list for QEMU `virt`, but must
    document how it evolves.

## Security considerations

### Threat model
- **Arbitrary memory access**: Malicious userspace service requests MMIO mapping outside device window
- **DMA attacks**: Compromised driver uses DMA to read/write arbitrary physical memory
- **Privilege escalation via MMIO**: Driver exploits MMIO access to compromise kernel
- **Device spoofing**: Attacker tricks kernel into providing MMIO cap for wrong device
- **Execute from MMIO**: Attacker attempts to execute code from mapped MMIO region

### Security invariants (MUST hold)
- MMIO mappings MUST be capability-gated (no ambient access)
- MMIO mappings MUST be bounded to the exact device BAR/window (no overmap)
- MMIO mappings MUST be **USER|RW only, NEVER executable** (W^X at hardware boundary)
- Only designated driver services may receive device MMIO capabilities
- Device capability distribution MUST be explicit and auditable
- DMA buffers (future) MUST be allocated from restricted memory regions

### DON'T DO
- DON'T allow any executable mappings of device MMIO regions
- DON'T grant MMIO capabilities to arbitrary services
- DON'T allow MMIO mappings outside the device's designated physical window
- DON'T expose device enumeration to untrusted services
- DON'T skip capability checks for "trusted" driver services

### Attack surface impact
- **Critical**: MMIO access is a kernel-userland trust boundary
- **Highest privilege**: Misconfigured MMIO access could compromise kernel
- **Requires security review**: Any changes to MMIO mapping must be reviewed

### Mitigations
- `CapabilityKind::DeviceMmio { base, len }` bounds physical window precisely
- `SYSCALL_MMIO_MAP` enforces **USER|RW, never EXEC** at syscall level
- Fixed, build-time device list for QEMU `virt` (no dynamic enumeration in bring-up)
- Capability distribution targets only the designated driver service (`netstackd`, `virtionetd`)
- W^X enforcement at page table level (no execute permission on device pages)

## Security proof

### Audit tests (negative cases)
- Command(s):
  - `cargo test -p neuron -- mmio_reject --nocapture`
- Required tests:
  - `test_reject_mmio_outside_window` — mapping beyond device bounds → denied
  - `test_reject_mmio_exec` — executable mapping attempt → denied
  - `test_reject_mmio_no_cap` — mapping without capability → denied

### Hardening markers (QEMU)
- `SELFTEST: mmio map ok` — legitimate mapping works
- `kernel: mmio denied (outside window)` — bounds enforcement works
- `kernel: mmio denied (exec attempt)` — W^X enforced

## Contract sources (single source of truth)

- Loader/mapping invariants: `docs/rfcs/RFC-0004-safe-loader-guards.md` (W^X and mapping safety expectations)
- IPC/cap model: `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md`

## Stop conditions (Definition of Done)

- Host/unit tests for capability + mapping invariants (as applicable).
- QEMU selftest marker proving a userspace driver can map its MMIO window and read a known virtio register:
  - `SELFTEST: mmio map ok`

## Current state

- Kernel cap kind exists: `CapabilityKind::DeviceMmio { base, len }` (bounded physical window).
- Kernel syscall exists: `SYSCALL_MMIO_MAP` enforcing **USER|RW** and **never EXEC** for device mappings.
- OS selftest exercises the path end-to-end and emits:
  - `SELFTEST: mmio map ok`
- Canonical QEMU harness now requires the marker (no silent “green”).
- **Bring-up caveat**: current virtio-net testing uses a temporary “selftest-client injection” path.
  For `TASK-0003` to complete, capability distribution must target the **networking owner service**
  (e.g. `netstackd` / `virtionetd`), not only selftests.

## Touched paths (allowlist)

- `source/kernel/neuron/` (minimal new capability kind and mapping syscall support)
- `source/libs/nexus-abi/` (userspace wrapper for the new capability/map primitive)
- `source/apps/selftest-client/` (MMIO map proof marker)
- `scripts/qemu-test.sh` (canonical marker harness)
- `docs/` (document the device access model + security invariants)

## Plan (small PRs)

1. Define a new capability kind for device MMIO windows (base, len, allowed flags).
2. Expose a syscall/wrapper to map a device MMIO cap into the caller’s AS (RW|USER, never X).
3. Wire a fixed virtio device list for QEMU `virt` for bring-up (cap distribution to the relevant service).
4. Add a selftest that maps and reads the virtio magic/version register deterministically.

## Acceptance criteria (behavioral)

- Userspace can map only the granted device MMIO window, not arbitrary addresses.
- Mapping is non-executable and respects W^X enforcement at the boundary.
- QEMU marker proves the mapping works.

## Evidence (to paste into PR)

- `cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` (marker present: `SELFTEST: mmio map ok`)
