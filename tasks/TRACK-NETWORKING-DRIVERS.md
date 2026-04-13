---
title: TRACK Networking Drivers (virtio-net etc.): contracts + gated roadmap for nexus-net + DSoftBus
status: Living
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Networking step 1: tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - Networking step 2: tasks/TASK-0004-networking-dhcp-icmp-dsoftbus-dual-node.md
  - Networking step 3: tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md
  - Device/MMIO access: tasks/TASK-0010-device-mmio-access-model.md
---

## Goal (track-level)

Keep networking “driver work” integrated with the networking + DSoftBus roadmap, focusing on:

- userspace-first virtio-net and networking primitives,
- deterministic proof (host tests + QEMU markers only after real behavior),
- bounded memory and rate limits (anti-flood),
- clean integration points for DSoftBus sessions/transports.

## Non-Goals

- This file is **not** an implementation task.
- No kernel changes are defined here.
- No attempt to build a generic full Linux network stack.

## Contracts (stable interfaces to design around)

- **Device access**: safe MMIO/IRQ model for virtio-net (gated).  
  Source: `TASK-0010`.
- **Networking API**: `nexus-net`-style UDP/TCP surface with bounded buffers and timers.  
  Source: `TASK-0003` / `TASK-0004`.
- **DSoftBus integration**:
  - discovery inputs (multicast/broadcast/mDNS later),
  - session transports (tcp now; quic/udp-sec later),
  - streams/mux layers above transport.

## Gates (RED / YELLOW / GREEN)

- **RED (blocking)**:
  - Userspace virtio-net requires a real MMIO capability/access path (TASK-0010) unless the OS already exposes a broker.
- **YELLOW (risky / drift-prone)**:
  - QEMU network backends can be flaky; keep proofs deterministic and bounded.
  - Multicast/mDNS behavior varies by backend; ensure host tests cover protocol logic deterministically.
- **GREEN (confirmed direction)**:
  - Host-first DSoftBus tests can validate discovery/auth/streams independent of OS bring-up.

## Phase map

- **Phase 0**: bring-up networking (static/DHCP), minimal sockets surface, DSoftBus local.
- **Phase 1**: discovery hardening, rate limits, dual-node/cross-VM harnesses.
- **Phase 2**: additional transports (udp-sec/quic), performance tuning, zero-copy buffers (later).

## Backlog (Candidate Subtasks)

- **CAND-NETDRV-001: virtio-net userspace frontend plumbing**  
  - **When**: now (once MMIO model exists)  
  - **Depends on**: `TASK-0010`  
  - **Proof idea**: QEMU markers “virtio-net up / iface up / DHCP bound”  
  - **Status**: candidate

- **CAND-NETDRV-010: zero-copy packet buffers (VMO/filebuffer)**  
  - **When**: later  
  - **Depends on**: VMO plumbing + driver kit contracts  
  - **Proof idea**: host tests for buffer lifetimes; QEMU marker for reduced copy path (instrumented)  
  - **Status**: candidate

## Extraction rules

- Only extract a candidate into a real `TASK-XXXX` when it can be proven with host tests and/or QEMU markers
  without relying on log greps as truth.
- After extraction, keep only a link and `Status: extracted → TASK-XXXX`.
