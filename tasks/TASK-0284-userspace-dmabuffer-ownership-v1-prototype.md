---
title: TASK-0284 Userspace driver optimization v1: ownership-based DMA buffer prototype (zero-copy)
status: Done 2026-09-24 — closed by TASK-0286 M1 P4b (absorbed 2026-09-22): `nexus_driverkit::DmaBuffer` (ownership typestate + cache maintenance at the transitions) over `nexus_abi::DmaVmo`, coherence from the device capability, user-mode Zicbom
owner: @runtime @drivers
created: 2026-01-09
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
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

## Closure (2026-09-24, TASK-0286 M1 P4b)

Delivered in the DriverKit contract crate instead of a new `userspace/` prototype crate — the
ownership type belongs next to the submit ring whose slots bound it, and it is real, not a
prototype: `nexus_driverkit::{DmaBuffer, InFlight, Direction, DmaMemory, CacheOps, Zicbom}`
over `nexus_abi::DmaVmo` (a VMO mapped for DMA, runs from `vmo_runs`). RFC-0098 C4 carries the
contract.

| Stop condition | How it holds | Proof |
|---|---|---|
| ownership transfer prevents double-submit | `for_device(self)` consumes the buffer | `compile_fail` doctest (a second `for_device` on the moved value) |
| the CPU cannot touch an in-flight buffer | `InFlight` exposes runs, never bytes | `compile_fail` doctest (`in_flight.bytes()`) |
| return gives exclusive ownership back | `InFlight::for_cpu(self) -> DmaBuffer` | host round trips in every direction; QEMU `SELFTEST: dma buffer ok (…)` |
| bounded in-flight count | a buffer is in flight on a ring slot; `SubmitRing` (`MAX_SLOTS`, backpressure) is the bound | `ring` host tests |
| zeroed before exposure | every VMO is zeroed at create (TASK-0286 P3a) | `KSELFTEST: vmo zero ok` |
| handles unforgeable | a `DmaVmo` is a VMO capability; its runs come only from `vmo_runs`, only to a device holder | `KSELFTEST: vmo runs ok (runs=2 deny=3)`; `dma_runs` reject tests |
| coherence (added by the absorption) | the device capability carries `dma-noncoherent`; clean to the device, flush from it and before the CPU reads; a non-coherent device without Zicbom is refused | 5 `dma` host tests incl. `test_reject_dma_buffer_for_an_unmaintainable_device`; smp1 + visible `SELFTEST: dma buffer ok (device=coherent block=64 runs=1)` with the instructions forced on |

The QEMU marker is `SELFTEST: dma buffer ok (…)` (the ledger's placeholder name
`SELFTEST: dmabuffer ownership ok` was never registered). First real consumers: the SDHCI
driver (B1.5, ADMA) and the display controller (B1.7) on the non-coherent board.
