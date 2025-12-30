---
title: TASK-0272 Thermal v0.1a (host-first): deterministic thermal model + thresholds + throttle requests
status: Draft
owner: @platform
created: 2025-12-30
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Authority & naming registry: tasks/TRACK-AUTHORITY-NAMING.md
  - Thermal OS wiring: tasks/TASK-0271-thermal-v0_1b-os-thermald-powerd-hooks-systemui-selftests.md
  - Power/Idle host core: tasks/TASK-0236-power-v1_0a-host-governor-wakelocks-residency-standby-deterministic.md
---

## Context

Before OS/QEMU wiring, we want deterministic semantics for thermal state transitions and “throttle requests”.
This makes the later `thermald` behavior testable and avoids policy drift with `powerd`.

## Goal

Host-first deliverables:

1. `userspace/libs/thermal-model`:
   - state machine for zones with thresholds: `nominal → warm → hot → critical`
   - deterministic hysteresis rules (explicit; no time-based heuristics)
   - input is `sample(temp_c)` events; output is state transitions + “actions”
2. `ThrottleRequest` contract:
   - `none | request_power_mode(frugal) | request_power_mode(perf)` (minimal v0.1)
   - deterministic mapping: `hot/critical` must request `frugal`
3. Deterministic tests (`cargo test`):
   - stable transitions for a fixed sample trace
   - hysteresis prevents oscillation for a fixed trace
   - bounded zone count and bounded event fanout

## Non-Goals

- Real sensor acquisition.
- Kernel integration.
- Complex DVFS tables or per-device throttling.

## Constraints / invariants (hard requirements)

- Deterministic: no wallclock.
- Bounded: zone count and event queue are bounded.
- No fake success: tests assert exact transition sequences.

## Touched paths (allowlist)

- `userspace/libs/thermal-model/**` (new)
- `tests/thermal_v0_1a_host/**` (new)
