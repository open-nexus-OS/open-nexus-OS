---
title: TASK-0256 Battery v0.9a (host-first): fuel-gauge model + charger state + thresholds + deterministic tests
status: Draft
owner: @runtime
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Power/idle baseline: tasks/TASK-0236-power-v1_0a-host-governor-wakelocks-residency-standby-deterministic.md
  - Testing contract: scripts/qemu-test.sh
  - Data formats rubric (JSON vs Cap'n Proto): docs/adr/0021-structured-data-formats-json-vs-capnp.md
---

## Context

We need a deterministic battery & charging foundation:

- fuel-gauge model (Coulomb counter with OCV clamp),
- charger state (unplugged/charging/full/discharging),
- thresholds (low/critical),
- persistence (cycles, last SoC).

The prompt proposes a fuel-gauge model and charger state. `TASK-0236`/`TASK-0237` already plan `powerd` (governor, wake-locks, residency). This task delivers the **host-first core** (fuel-gauge model, charger state, thresholds) that feeds into `powerd`'s low/critical actions.

## Goal

Deliver on host:

1. **Fuel-gauge model library** (`userspace/libs/battery-model/`):
   - simple Coulomb counter with OCV clamp between `voltage_empty_mv..voltage_full_mv`
   - temperature stable 25 °C unless injected
   - deterministic model (stable given the same current draw sequence)
2. **Charger state library** (`userspace/libs/battery-charger/`):
   - states: `unplugged`, `charging`, `full`, `discharging`
   - when `plug=true`, state `charging` until 100%, then `full`; unplug sets `discharging`
   - deterministic transitions
3. **Thresholds library** (`userspace/libs/battery-thresholds/`):
   - `low_pct` (default 15%), `critical_pct` (default 5%)
   - threshold events (low/critical/thermal hot/cold)
   - deterministic threshold detection
4. **Persistence library** (`userspace/libs/battery-persistence/`):
   - store cycles and last SoC under `state:/battery/state.nxs` (Cap'n Proto snapshot; canonical; gated on `/state`)
   - deterministic state serialization (byte-stable)
5. **Host tests** proving:
   - Coulomb model: with fixed current draw, SoC drops linearly and clamps at 0%/100%
   - thresholds: set SoC to 16% then inject −2% → expect `low`; drop to 5% → expect `critical`
   - plug state: plug on → `charging` until 100% then `full`; unplug → `discharging`
   - persistence: cycles counter increments predictably after full discharge/charge loop; state survives restart

## Non-Goals

- OS/QEMU integration (deferred to v0.9b).
- Real hardware (QEMU/null fuel-gauge/charger only).
- Power actions (handled by `TASK-0237` via `powerd` hooks).

## Constraints / invariants (hard requirements)

- **No duplicate battery authority**: This task provides battery model library. `TASK-0237` plans SystemUI battery UI. Both should share the same battery state contract to avoid drift.
- **Determinism**: fuel-gauge model, charger state, thresholds, and persistence must be stable given the same inputs.
- **Bounded resources**: battery state is bounded; cycles counter is bounded.
- **Persistence gating**: persistence requires `/state` (`TASK-0009`) or equivalent. Without `/state`, persistence must be disabled or explicit `stub/placeholder` (no "written ok" claims).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (battery authority drift)**:
  - Do not create parallel battery state contracts. This task provides battery model library. `TASK-0237` (SystemUI battery UI) should consume the same battery state contract to avoid drift.
- **YELLOW (persistence determinism)**:
  - Persistence must use deterministic serialization (byte-stable snapshots; fixed mtime/uid/gid if writing files).

## Contract sources (single source of truth)

- Testing contract: `scripts/qemu-test.sh`
- Power/idle baseline: `TASK-0236`/`TASK-0237` (powerd hooks for low/critical actions)
- Persistence: `TASK-0009` (prerequisite for `/state`)

## Stop conditions (Definition of Done)

### Proof (Host) — required

`cargo test -p battery_v0_9_host` green (new):

- Coulomb model: with fixed current draw, SoC drops linearly and clamps at 0%/100%
- thresholds: set SoC to 16% then inject −2% → expect `low`; drop to 5% → expect `critical`
- plug state: plug on → `charging` until 100% then `full`; unplug → `discharging`
- persistence: cycles counter increments predictably after full discharge/charge loop; state survives restart

## Touched paths (allowlist)

- `userspace/libs/battery-model/` (new)
- `userspace/libs/battery-charger/` (new)
- `userspace/libs/battery-thresholds/` (new)
- `userspace/libs/battery-persistence/` (new)
- `schemas/battery_v0_9.schema.json` (new)
- `tests/battery_v0_9_host/` (new)
- `docs/power/battery_v0_9.md` (new, host-first sections)

## Plan (small PRs)

1. **Fuel-gauge model + charger state**
   - fuel-gauge model library (Coulomb counter + OCV clamp)
   - charger state library
   - host tests

2. **Thresholds + persistence**
   - thresholds library
   - persistence library (gated on `/state`)
   - host tests

3. **Schema + docs**
   - `schemas/battery_v0_9.schema.json`
   - host-first docs

## Acceptance criteria (behavioral)

- Fuel-gauge model: with fixed current draw, SoC drops linearly and clamps at 0%/100%.
- Thresholds: set SoC to 16% then inject −2% → expect `low`; drop to 5% → expect `critical`.
- Plug state: plug on → `charging` until 100% then `full`; unplug → `discharging`.
- Persistence: cycles counter increments predictably after full discharge/charge loop; state survives restart.
