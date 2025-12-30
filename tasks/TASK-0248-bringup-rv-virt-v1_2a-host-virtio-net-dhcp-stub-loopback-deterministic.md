---
title: TASK-0248 RISC-V Bring-up v1.2a (host-first): virtio-net frontend core + DHCP stub + loopback + deterministic tests
status: Draft
owner: @kernel
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Networking baseline (smoltcp): tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need virtio-net support for bring-up networking:

- virtio-net frontend (deterministic TAP backend),
- DHCP stub (in-process, deterministic),
- loopback support.

The prompt proposes userspace virtio-net with deterministic TAP backend and a minimal DHCP stub. `TASK-0003` already plans virtio-net + smoltcp for DSoftBus. This task delivers a **lightweight alternative** for bring-up that focuses on deterministic DHCP stub and loopback, complementing the smoltcp-based system.

## Goal

Deliver on host:

1. **Virtio-net frontend library** (`userspace/libs/virtio-net/` or `source/drivers/net/virtio/`):
   - virtio-mmio net frontend (legacy or modern minimal subset)
   - feature negotiation: no multi-queue, no mergeable buffers, checksum offload off (for determinism)
   - TX/RX rings with fixed-size descriptors; RX pre-posted buffers from a slab
   - deterministic MAC (e.g., `02:00:00:00:00:01`) exposed in config
   - deterministic ring math (wrap-around handling)
2. **DHCP stub library** (`userspace/libs/dhcp-stub/`):
   - single-host deterministic lease by exchanging frames with built-in stub server
   - DISCOVER → OFFER/ACK with fixed lease (e.g., 10.0.2.15/24, gw 10.0.2.2, dns 10.0.2.3)
   - entire handshake remains in-process to avoid flakiness
   - deterministic renewal (idempotent)
3. **Loopback support**:
   - loopback `lo` (127.0.0.1/8) with raw socket shim
   - minimal ICMP echo reply on `lo`
   - deterministic send/recv path
4. **Host tests** proving:
   - virtio-net rings: descriptor wraparound & IRQ kick/ack with fake MMIO backend
   - DHCP stub: request → deterministic address/gw/dns; renewal idempotent
   - loopback: ICMP echo on `lo` returns within bound; raw send/recv path verified

## Non-Goals

- Full smoltcp stack (handled by `TASK-0003`).
- Multi-queue or advanced virtio features (minimal subset only).
- Real DHCP server (in-process stub only).

## Constraints / invariants (hard requirements)

- **No duplicate virtio-net authority**: This task provides a lightweight virtio-net frontend. `TASK-0003` uses smoltcp for DSoftBus. Both can coexist if they share MMIO access, or this task must explicitly replace smoltcp as the canonical system.
- **Determinism**: virtio-net rings, DHCP stub, and loopback must be stable given the same inputs.
- **Bounded resources**: DHCP stub is in-process only; loopback is bounded.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (virtio-net authority drift)**:
  - Do not create a parallel virtio-net implementation that conflicts with `TASK-0003` (smoltcp). If both coexist, they must share MMIO access and not conflict. Document the relationship explicitly.
- **YELLOW (DHCP stub vs real DHCP)**:
  - DHCP stub is in-process and deterministic, not a real DHCP server. Document this explicitly and ensure policy language does not claim otherwise.

## Contract sources (single source of truth)

- Testing contract: `scripts/qemu-test.sh`
- Networking baseline: `TASK-0003` (virtio-net + smoltcp)

## Stop conditions (Definition of Done)

### Proof (Host) — required

`cargo test -p bringup_rv_virt_v1_2_host` green (new):

- virtio-net rings: descriptor wraparound & IRQ kick/ack with fake MMIO backend
- DHCP stub: request → deterministic address/gw/dns; renewal idempotent
- loopback: ICMP echo on `lo` returns within bound; raw send/recv path verified

## Touched paths (allowlist)

- `userspace/libs/virtio-net/` (new; or extend `source/drivers/net/virtio/`)
- `userspace/libs/dhcp-stub/` (new)
- `userspace/libs/loopback/` (new)
- `tests/bringup_rv_virt_v1_2_host/` (new)
- `docs/bringup/virt_net_v1_2.md` (new, host-first sections)

## Plan (small PRs)

1. **Virtio-net frontend library**
   - virtio-mmio net frontend (minimal subset)
   - ring math (descriptor/avail/used)
   - host tests

2. **DHCP stub + loopback**
   - DHCP stub library (in-process)
   - loopback support
   - host tests

3. **Docs**
   - host-first docs

## Acceptance criteria (behavioral)

- Virtio-net frontend library handles ring math correctly.
- DHCP stub produces deterministic leases.
- Loopback works correctly.
