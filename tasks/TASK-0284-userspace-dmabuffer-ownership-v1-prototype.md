---
title: TASK-0284 Userspace driver optimization v1: ownership-based DMA buffer prototype (zero-copy)
status: Draft
owner: @runtime @drivers
created: 2026-01-09
links:
  - Vision: docs/agents/VISION.md
  - Drivers/accelerators track: tasks/TRACK-DRIVERS-ACCELERATORS.md
  - Device/MMIO access: tasks/TASK-0010-device-mmio-access-model.md
  - VMO plumbing: tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
---

## Context

To minimize kernel code and driver LOC while keeping performance high, we want a “thin kernel, safe userspace driver” split:

- kernel provides capability-gated MMIO windows and VMO primitives,
- userspace drivers do register programming and command submission,
- bulk data uses zero-copy buffers.

Rust’s ownership model can encode buffer lifecycle in a way that eliminates refcounting bugs common in C (double-free, UAF).

## Goal

Create a host-first prototype `DmaBuffer` abstraction that models:

- “buffer owned by CPU” vs “buffer in-flight on device”
- ownership transfer via `Fence`-like handle
- bounded, deterministic APIs usable by device-class services

## Non-Goals

- Real DMA isolation (IOMMU/GPU-MMU) – future work.
- Kernel changes – prototype can run host-only and be OS-gated.

## Constraints / invariants (hard requirements)

- No secrets in logs.
- Bounded buffer sizes and bounded in-flight buffers.
- Deterministic tests and stable markers.

## Security considerations

### Threat model

- **Use-after-free**: buffer reused while device still reads/writes
- **Double-submit**: same buffer submitted twice concurrently
- **Information leakage**: uninitialized buffer contents exposed to another client

### Security invariants (MUST hold)

- Buffer cannot be accessed by CPU while owned by fence/in-flight
- Buffers are zeroed or explicitly initialized before exposure to another client
- All buffer IDs/handles are unforgeable in the OS path (capability-backed when available)

## Stop conditions (Definition of Done)

### Proof (Host) — required

- Deterministic tests prove:
  - ownership transfer prevents double-submit
  - fence return returns exclusive ownership
  - bounded in-flight count is enforced

### Proof (OS/QEMU) — optional/gated

- Only once a real device-class service exists:
  - `SELFTEST: dmabuffer ownership ok`

## Touched paths (allowlist)

- `userspace/` (new prototype crate)
- `tasks/TRACK-DRIVERS-ACCELERATORS.md` (link as extracted)
