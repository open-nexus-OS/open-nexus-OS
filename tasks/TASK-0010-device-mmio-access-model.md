---
title: TASK-0010 Device access model v1: safe userspace MMIO for virtio devices (enables virtio-net/virtio-blk)
status: Draft
owner: @kernel-team @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - RFC: docs/rfcs/RFC-0005-kernel-ipc-capability-model.md
  - Unblocks: tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - Unblocks: tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
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
  - This task **requires kernel work**. If “kernel untouched” is absolute, then userspace virtio drivers
    must be deferred or replaced with a different backend (e.g., host-provided VMO block service) and the
    vision “userspace drivers” is not achievable on QEMU `virt`.
- **YELLOW (risky / likely drift / needs follow-up)**:
  - Device enumeration: we can start with a fixed, build-time wired device list for QEMU `virt`, but must
    document how it evolves.

## Contract sources (single source of truth)

- Loader/mapping invariants: `docs/rfcs/RFC-0004-safe-loader-guards.md` (W^X and mapping safety expectations)
- IPC/cap model: `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md`

## Stop conditions (Definition of Done)

- Host/unit tests for capability + mapping invariants (as applicable).
- QEMU selftest marker proving a userspace driver can map its MMIO window and read a known virtio register:
  - `SELFTEST: mmio map ok`

## Touched paths (allowlist)

- `source/kernel/neuron/` (minimal new capability kind and mapping syscall support)
- `source/libs/nexus-abi/` (userspace wrapper for the new capability/map primitive)
- `source/apps/selftest-client/` (MMIO map proof marker)
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
