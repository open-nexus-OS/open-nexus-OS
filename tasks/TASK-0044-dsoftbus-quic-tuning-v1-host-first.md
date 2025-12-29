---
title: TASK-0044 DSoftBus QUIC tuning v1: pacing + congestion selection + mux WFQ priorities (host-first, OS-gated)
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - DSoftBus QUIC baseline: tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md
  - Mux v2 baseline: tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md
  - Discovery/authz hardening: tasks/TASK-0030-dsoftbus-discovery-authz-hardening-mdns-ttl-acl-ratelimit.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We want more realistic performance under load while keeping the system small and testable:

- QUIC transport tuning: packet pacing and congestion control selection.
- Mux v2 improvements: stream priorities and a fair scheduler.
- Keep TCP fallback intact.

Repo reality today:

- Host DSoftBus backend exists; OS backend is still a placeholder.
- QUIC on OS is blocked/gated (no_std feasibility); host QUIC is the primary environment for this work.
- Mux v2 is planned but not implemented yet.

Therefore this task is **host-first** and **OS-gated**.

## Goal

On host builds, prove:

- QUIC pacing can be enabled/disabled and is reflected in behavior under loss/load.
- Congestion control selection works (default Reno; optional BBR-lite where feasible).
- Mux v2 prioritization improves control/interactive latency under mixed bulk load while preserving throughput.
- If peer lacks priority support, behavior falls back deterministically.

## Non-Goals

- Kernel changes.
- Claiming OS/QEMU QUIC tuning markers before OS QUIC exists.
- Perfect, universally optimal congestion control; v1 focuses on robust defaults + knobs.

## Constraints / invariants (hard requirements)

- Kernel untouched.
- Deterministic tests: prefer structural assertions and coarse ratios over exact timing.
- Bounded memory and bounded queues.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake success markers.

## Red flags / decision points

- **RED (gating)**:
  - Requires host QUIC backend implementation (TASK-0021) and mux v2 implementation (TASK-0020) before the full story can be proven.
- **YELLOW (BBR-lite feasibility)**:
  - If the chosen QUIC stack does not provide a safe pluggable BBR-lite, keep Reno as the only supported CC in v1 and document the limitation.
- **YELLOW (test flakiness)**:
  - Load tests can become flaky if based on wall-clock latency thresholds. Use deterministic workload generators and validate relative ordering/ratios.

## Stop conditions (Definition of Done)

### Proof (Host) — required

New deterministic host tests (`tests/dsoftbus_quic_tuning_host/`):

- Mixed workload:
  - one control stream (small RPCs)
  - one high stream (interactive bursts)
  - multiple normal streams (bulk chunks)
  - one low stream (background/backfill)
- Assertions:
  - control/high p50 latency improves vs FIFO under concurrent bulk load (coarse threshold, e.g. ≥30%).
  - throughput regression under no-loss stays within a small bound.
  - with a deterministic loss emulator, pacing improves stability vs pacing off.
- Fallback:
  - priority caps off → FIFO ordering (deterministic).

### Proof (OS / QEMU) — gated

Only once OS QUIC is real (future) should we add QEMU markers for pacing/cc/priorities.

## Touched paths (allowlist)

- `userspace/dsoftbus/` (transport/quic host, mux v2 scheduler)
- `tests/` (host tuning/load tests)
- `docs/distributed/dsoftbus-quic.md` (knobs, defaults, tuning notes)
- `docs/distributed/dsoftbus-mux.md` (priority scheduler notes)

## Plan (small PRs)

1. **QUIC knobs (host)**
   - Env/config:
     - `QUIC_PACE=on|off` (default on)
     - `QUIC_CC=reno|bbr-lite` (default reno; bbr-lite optional)
     - `QUIC_INIT_CWND_PKTS` (default 10)
   - Emit deterministic markers when enabled on host runs (for tests).

2. **Mux v2 priorities + scheduler**
   - Define priority classes: control/high/normal/low with weights (e.g. 8/4/2/1).
   - Implement a weighted fair queue among ready streams, respecting existing window/backpressure.
   - Add a negotiation bit (`caps.pri=1`) and deterministic fallback.

3. **Host load tests**
   - Deterministic workload generator + optional deterministic loss emulator.
   - Validate improvements and fallback behavior.

4. **Docs**
   - Explain knobs and default policy.
