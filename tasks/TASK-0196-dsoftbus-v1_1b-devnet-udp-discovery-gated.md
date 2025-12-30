---
title: TASK-0196 DSoftBus v1.1b (devnet-gated): UDP discovery + loopback/udp mode switch + deterministic beacons + tests (host-first), OS gated
status: Draft
owner: @networking
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - DSoftBus v1 localSim OS slice: tasks/TASK-0158-dsoftbus-v1b-os-consent-policy-registry-share-demo-cli-selftests.md
  - DSoftBus v1.1 secure channels: tasks/TASK-0195-dsoftbus-v1_1a-host-secure-channels-encrypted-streams-share.md
  - Networking v1 devnet gating: tasks/TASK-0193-networking-v1a-host-devnet-tls-fetchd-integration.md
  - OS networking prerequisites: tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - Device MMIO access model: tasks/TASK-0010-device-mmio-access-model.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We want an optional UDP-based discovery mode for DSoftBus that is:

- disabled by default (offline determinism),
- enabled only when `network.devnet.enabled=true`,
- and testable deterministically on host using localhost UDP (no LAN required).

OS/QEMU enablement is gated on OS networking bring-up (`TASK-0003` + `TASK-0010`).

## Goal

Deliver:

1. Mode switch:
   - `dsoftbus.mode = loopback|udp`
   - if devnet is off, force loopback regardless of requested mode
2. UDP discovery beacons (host-first):
   - bind to `127.0.0.1` and send periodic beacons to a configured port/mcast (localhost-only acceptable)
   - beacon contains:
     - peer fingerprint, service list, protocol version
   - TTL aging and deterministic lastSeen based on injected clock in tests
3. Deterministic host tests (`tests/dsoftbus_v1_1_udp_host/`):
   - devnet off → udp mode request forced to loopback
   - devnet on → beacon received; peer appears; TTL aging works deterministically
4. OS gating:
   - OS selftest only asserts “devnet off forces loopback” unless OS networking is unblocked
   - never claim `udp ok` markers without real OS UDP sockets

## Non-Goals

- Kernel changes in this task.
- Full LAN/mDNS discovery correctness.

## Constraints / invariants (hard requirements)

- Offline by default; determinism preserved.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **RED (OS networking dependency)**:
  - UDP mode on OS requires real networking (virtio-net + MMIO access). Until then, OS must remain loopback-only.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p dsoftbus_v1_1_udp_host -- --nocapture`

- **Proof (QEMU)**:
  - required:
    - `SELFTEST: bus mode gate ok`
  - optional (only if unblocked):
    - `SELFTEST: bus udp discover ok`

## Touched paths (allowlist)

- `source/services/dsoftbusd/` (mode switch + udp discovery)
- `tests/dsoftbus_v1_1_udp_host/`
- `schemas/dsoftbus.schema.json`
- `docs/dsoftbus/overview.md`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. host-first UDP beacon + tests
2. devnet gating + docs
3. OS marker gating (loopback-only unless unblocked)

## Acceptance criteria (behavioral)

- Host tests deterministically prove localhost UDP discovery under devnet; OS remains honest and loopback-only unless networking is unblocked.

