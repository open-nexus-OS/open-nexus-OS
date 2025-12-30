---
title: TASK-0267 Kernel IPC v1: framed channels + capability transfer (QEMU-proof)
status: Draft
owner: @kernel
created: 2025-12-30
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - RFC-0005 IPC/capability model: docs/rfcs/RFC-0005-kernel-ipc-capability-model.md
  - Keystone gates: tasks/TRACK-KEYSTONE-GATES.md
  - Zero-copy VMO plumbing: tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - Sandboxing v1 (CapFd rights): tasks/TASK-0039-sandboxing-v1-vfs-namespaces-capfd-manifest.md
  - sysfilter baseline: tasks/TASK-0188-kernel-sysfilter-v1-task-profiles-rate-buckets.md
---

## Context

The architecture is process-per-service. Until IPC + capability transfer exist in the kernel, many “services”
cannot be real and QEMU proofs will be forced to rely on in-proc stubs.

This task closes Keystone Gate 1 by turning RFC-0005 into a minimal, deterministic, QEMU-proof kernel slice.

## Goal

Implement the **minimum viable kernel IPC** surface (RFC-0005 aligned):

1. **Channel endpoints**:
   - create a paired channel (two endpoints).
   - endpoints are capabilities with rights.
2. **Framed messages**:
   - deterministic framing: `(len, bytes)` with bounded max size.
   - send/recv are explicit about partial/oversize errors (no silent truncation).
3. **Capability transfer**:
   - a message can carry **N capability handles** (bounded N).
   - transfer consumes sender handle(s) or duplicates based on explicit semantics (RFC-defined).
4. **Rights enforcement**:
   - kernel enforces rights on capabilities (basic read/write/transfer/map).
5. **Deterministic scheduling impact**:
   - operations must not rely on wallclock timing; tests use bounded loops and explicit yields.

## Non-Goals

- A full Cap’n Proto RPC runtime in-kernel (RPC stays userspace).
- Cross-machine “remote caps” (explicitly not in scope).
- High-performance zero-copy data plane beyond what `TASK-0031` already plans (this is control-plane correctness first).

## Constraints / invariants (hard requirements)

- **Determinism**: IPC tests must pass reliably in QEMU with stable markers.
- **Bounded resources**:
  - max frame size,
  - max in-flight frames per endpoint,
  - max transferred caps per message,
  - backpressure errors are explicit and testable.
- **No fake success**: every “ok” marker must correspond to a real cross-process send/recv + cap transfer.

## Proof (QEMU) — required markers

Add kernel selftests (or userspace smoke via a tiny test service) that emit:

- `SELFTEST: ipc v1 channel create ok`
- `SELFTEST: ipc v1 sendrecv ok bytes=<N>`
- `SELFTEST: ipc v1 capxfer ok n=<N>`
- `SELFTEST: ipc v1 backpressure ok`

## Unblocks

- Real cross-process services (`contentd`, `grantsd`, `windowd`, `inputd`, etc.).
- CapFd authenticity patterns in sandboxing (`TASK-0039`).
- VMO transfer proofs (`TASK-0031`).

## Touched paths (allowlist)

- `kernel/**` (IPC + capability table)
- `tests/**` or `userspace/tests/**` (QEMU selftests + markers)
- `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md` (errata only, if needed)
