---
title: TASK-0021 DSoftBus QUIC v1: host QUIC transport (quinn) + OS UDP scaffold (disabled) + TCP fallback
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - ADR: docs/adr/0005-dsoftbus-architecture.md
  - DSoftBus overview: docs/distributed/dsoftbus-lite.md
  - Depends-on (OS networking): tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - Depends-on (OS streams): tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md
  - Depends-on (mux over transport): tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We want an optional QUIC/UDP transport alongside TCP for DSoftBus, without breaking the default
bring-up path. The best approach is:

- implement QUIC transport **host-first** (quinn),
- keep OS QUIC as a **disabled-by-default scaffold** until OS networking exists,
- provide a deterministic runtime selection with clean fallback to TCP.

## Goal

Prove:

- Host: QUIC session works and can carry mux traffic (if mux exists), including negative cases.
- OS/QEMU: default path remains green; OS reports deterministic “QUIC disabled → fallback TCP” markers.

## Non-Goals

- Enabling QUIC on OS by default (future step once nexus-net UDP exists and is stable).
- Datagram-mode protocols.
- Kernel changes.

## Constraints / invariants (hard requirements)

- **Kernel untouched**.
- **Default stays green**: QUIC must not destabilize existing TCP bring-up.
- **Deterministic selection**: `auto` tries QUIC then falls back; failures are logged/marked clearly.
- **No fake success**: do not emit “quic ok” markers unless QUIC was actually used.

## Red flags / decision points

- **RED**:
  - OS DSoftBus backend is currently a placeholder until networking tasks land. OS QUIC must remain off by default and must not claim support.
- **YELLOW**:
  - Certificate/identity model: v1 can use ephemeral self-signed certs for host tests, but long-term should bind to device identity keys.
  - Async runtime: quinn is async; keep the host tests isolated and avoid pulling async into OS bring-up.

## Contract sources (single source of truth)

- DSoftBus traits: `userspace/dsoftbus`
- Mux v2 task: TASK-0020 (runs over any transport)
- QEMU marker contract: `scripts/qemu-test.sh`

## Stop conditions (Definition of Done)

### Proof (Host)

- Deterministic host tests:
  - QUIC connect + bidir stream + small echo (and/or mux smoke if available)
  - Negative: wrong ALPN / cert rejection path fails cleanly
  - Fallback: `auto` with QUIC unavailable selects TCP

### Proof (OS / QEMU)

- Default QEMU run passes and includes:
  - `dsoftbus: quic os disabled (fallback tcp)`
  - `SELFTEST: quic fallback ok`
  - and `dsoftbusd: transport selected tcp` (or equivalent selection marker)

Notes:

- Any postflight must only delegate to canonical harness/tests.

## Touched paths (allowlist)

- `userspace/dsoftbus/` (transport abstraction + selection)
- `source/services/dsoftbusd/` (selection + discovery advertise)
- `tests/` (host QUIC tests)
- `source/apps/selftest-client/` (fallback marker)
- `docs/distributed/`
- `scripts/qemu-test.sh` (accept fallback markers)

## Plan (small PRs)

1. Add a transport abstraction layer (`TransportKind { Tcp, Quic }`) and runtime selection (`auto|tcp|quic`).
2. Implement host QUIC endpoint via quinn (single bidir stream per session).
3. Add OS QUIC scaffold behind a runtime flag; by default emit `quic os disabled` and fall back to TCP.
4. Integrate selection into dsoftbusd discovery payload and session establishment.
5. Add host tests + OS selftest markers (fallback path only until OS networking exists).
6. Docs: transport selection, security notes, and enablement plan.

## Follow-ups

- QUIC tuning (pacing/CC) + mux priorities load testing: see `TASK-0044`.
