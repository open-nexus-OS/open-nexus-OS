---
title: TASK-0236 Power/Idle v1.0a (host-first): deterministic governor + wake locks + residency metrics + app standby + tests
status: Draft
owner: @runtime
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Timer coalescing baseline (timed): tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md
  - Ability lifecycle (BG detection): tasks/TASK-0235-ability-v1_1b-os-appmgrd-extension-samgr-hooks-fgbg-policies-selftests.md
  - Media sessions (active playback): tasks/TASK-0155-media-ux-v1a-host-mediasessd-focus-nowplaying-artcache.md
---

## Context

We need a deterministic power/idle management system with:

- governor modes (balanced/performance/frugal),
- wake locks (partial/full),
- idle residency metrics,
- app standby policies.

The prompt proposes `powerd`, `standbyd`, and wake locks. This task delivers the **host-first core** (deterministic state machine, policy logic, tests) before OS/QEMU wiring.

## Goal

Deliver on host:

1. Power governor library (`userspace/libs/power-governor/`):
   - modes: `balanced`, `performance`, `frugal`
   - coalescing policy mapping (delegated to `timed` from `TASK-0013`)
   - idle level hints (L0/L1/L2) as policy output
2. Wake lock manager (`userspace/libs/wakelock/`):
   - `partial` (blocks L2 deep idle)
   - `full` (additionally keeps screen awake)
   - reference counting and deterministic release
3. Idle residency tracker:
   - rolling window (60s) with L0/L1/L2 percentages
   - deterministic sampling (injectable time source in tests)
4. App standby policy:
   - BG inactivity threshold (`bg_inactive_min`)
   - standby state (deny sensors/network, floor timers to `timer_floor_ms`)
   - deterministic transitions
5. Host tests proving:
   - governor mode changes affect coalescing hints correctly
   - wake locks block L2 entry deterministically
   - residency % matches expected tolerance
   - standby entry/exit works correctly

## Non-Goals

- Kernel changes (idle hooks deferred to v1.0b).
- OS/QEMU markers (deferred to v1.0b).
- Real battery hardware (deterministic fixture only).

## Constraints / invariants (hard requirements)

- **No duplicate timer authority**: wake locks and standby delegate to `timed` (`TASK-0013`), not a new timer service.
- **Determinism**: governor decisions, wake lock state, and residency calculations must be stable given the same input sequence.
- **Bounded state**: residency history is bounded (rolling window, not unbounded log).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (timer authority drift)**:
  - Do not introduce a new timer service. Reuse `timed` from `TASK-0013` for coalescing and alarm scheduling.
- **YELLOW (residency determinism)**:
  - Residency sampling must use injectable time source in tests (not `std::time::SystemTime`).

## Contract sources (single source of truth)

- Timer coalescing baseline: `TASK-0013`
- Ability lifecycle (BG detection): `TASK-0235`
- Media sessions (active playback): `TASK-0155`

## Stop conditions (Definition of Done)

### Proof (Host) — required

`cargo test -p power_v1_0_host` green (new):

- governor: mode changes produce correct coalescing hints (balanced=20ms, frugal=50ms, performance=0ms)
- wake locks: `partial` blocks L2; `full` additionally signals screen-on; release allows L2
- residency: simulated idle periods produce expected L1/L2 percentages within tolerance
- standby: BG app idles for threshold → enters standby; timer requests floored to ≥ `timer_floor_ms`; FG exit → standby cleared

## Touched paths (allowlist)

- `userspace/libs/power-governor/` (new)
- `userspace/libs/wakelock/` (new)
- `userspace/libs/power-residency/` (new)
- `userspace/libs/standby/` (new)
- `schemas/power_v1_0.schema.json` (new)
- `tests/power_v1_0_host/` (new)
- `docs/power/overview.md` (new, host-first sections)

## Plan (small PRs)

1. **Governor + wake locks**
   - governor mode enum + coalescing policy mapping
   - wake lock manager (partial/full, ref counting)
   - host tests for mode changes and wake lock blocking

2. **Residency tracker**
   - rolling window with L0/L1/L2 percentages
   - injectable time source for tests
   - host tests for residency calculation

3. **Standby policy**
   - BG inactivity detection + standby state
   - timer floor enforcement
   - host tests for standby entry/exit

4. **Schema + docs**
   - `schemas/power_v1_0.schema.json`
   - host-first docs

## Acceptance criteria (behavioral)

- Governor modes produce correct coalescing hints.
- Wake locks block L2 entry deterministically.
- Residency % matches expected tolerance.
- Standby entry/exit works correctly with timer floor enforcement.
