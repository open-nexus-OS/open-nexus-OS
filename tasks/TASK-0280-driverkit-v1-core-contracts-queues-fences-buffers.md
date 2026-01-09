---
title: TASK-0280 DriverKit v1 core contracts: queues, fences, buffers (cross-device)
status: Draft
owner: @runtime @drivers
created: 2026-01-09
links:
  - Vision: docs/agents/VISION.md
  - Drivers/accelerators track: tasks/TRACK-DRIVERS-ACCELERATORS.md
  - ADR: docs/adr/0018-driverkit-abi-versioning-and-stability.md
  - Depends-on (MMIO caps): tasks/TASK-0010-device-mmio-access-model.md
  - Depends-on (VMO plumbing): tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - Depends-on (QoS/timers): tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md
---

## Context

To reduce kernel complexity and driver LOC while retaining performance, Open Nexus OS needs a stable, cross-device “DriverKit” contract used by GPU/NPU/VPU/Audio/Camera/ISP/Storage/Networking accelerators.

This task extracts `CAND-DRV-000` into a real task with proofs and explicit v1 boundaries.

## Goal

Define and implement a minimal **DriverKit v1 contract** (host-first; OS-gated) for:

- **queues/submit** (bounded in-flight work, backpressure),
- **timeline fences** (waitsets + deadlines),
- **buffers** (VMO/filebuffer handles, slices, budgets),
- **fault/reset** semantics (audited).

## Non-Goals

- A full GPU API (that belongs to a separate SDK track).
- Vendor-specific command encoding formats.
- Kernel driver logic.

## Constraints / invariants (hard requirements)

- Deterministic behavior and deterministic proofs (no timing-flaky tests).
- Bounded memory and bounded queues (hard caps).
- No fake success markers.
- Security boundaries explicit: policy is `policyd`; kernel enforces held capability rights.

## Security considerations

### Threat model

- **Command injection**: malformed submit payloads used to crash driver service
- **Resource exhaustion**: unbounded queue depth or fence waits causing DoS
- **Information leakage**: exposing device topology or other clients’ activity

### Security invariants (MUST hold)

- All DriverKit operations are bounded (sizes, queue depth, wait limits)
- Client identity binding uses `sender_service_id` (no payload identity strings)
- Driver service must be crash-contained and restartable
- DriverKit must not expose raw MMIO or DMA outside capability gates

### DON'T DO

- DON'T accept unbounded command buffers or unbounded fence waits
- DON'T log secrets or device private keys
- DON'T treat DriverKit as a policy authority (that stays in `policyd`)

## Stop conditions (Definition of Done)

### Proof (Host) — required

- Deterministic tests prove:
  - bounded queues and stable overflow behavior
  - fence wait semantics with deadlines
  - stable error mapping and reason codes

### Proof (OS/QEMU) — gated

Once relevant services exist, add QEMU markers:

- `driverkit: ready`
- `SELFTEST: driverkit submit ok`
- `SELFTEST: driverkit backpressure ok`
- `SELFTEST: driverkit fence wait ok`

## Touched paths (allowlist)

- `userspace/` (new driverkit core crate)
- `source/services/` (device-class services integrate it)
- `docs/adr/0018-driverkit-abi-versioning-and-stability.md`
- `tasks/TRACK-DRIVERS-ACCELERATORS.md`

## Plan (small PRs)

1. Define v1 data structures and error codes (bounded, versioned).
2. Implement host-only backend with deterministic tests.
3. Add OS wiring only when device-class services exist (QEMU markers gated).
