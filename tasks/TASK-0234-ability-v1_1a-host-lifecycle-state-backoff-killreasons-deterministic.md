---
title: TASK-0234 Ability/Lifecycle v1.1a (host-first): deterministic lifecycle state machine + backoff/crash-loop + kill reasons + tests
status: Draft
owner: @runtime
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - App lifecycle baseline (appmgrd): tasks/TASK-0065-ui-v6b-app-lifecycle-notifications-navigation.md
  - Execd (spawner authority): tasks/TASK-0001-runtime-roles-and-boundaries.md
  - Intent routing (intentsd): tasks/TASK-0126-share-v2a-intentsd-registry-dispatch-policy-host.md
  - QoS/timer slack baseline: tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md
  - OOM watchdog (kill integration): tasks/TASK-0228-oomd-v1-deterministic-watchdog-cooperative-memstat-samgr-kill.md
---

## Context

We need a robust, deterministic process/ability lifecycle system with:

- explicit state machine (stopped/starting/runningFg/runningBg/stopping/crashed/backoff),
- exponential backoff with crash-loop detection,
- kill reasons plumbed end-to-end,
- FG/BG policy enforcement (CPU budgets, timer slack).

The prompt proposes a new `abilityd` service, but we already have `appmgrd` planned (`TASK-0065`) and a stub `abilitymgr` in the repo. To avoid authority drift, this task extends the **`appmgrd` lifecycle model** (not introducing a competing `abilityd`).

## Goal

Deliver on host:

1. Deterministic lifecycle state machine library (`userspace/libs/ability-lifecycle/`):
   - states: `stopped`, `starting`, `runningFg`, `runningBg`, `stopping`, `crashed`, `backoff`
   - deterministic transitions with bounded timeouts
   - kill reasons enum: `none`, `crash`, `policy`, `oom`, `signal`, `request`
2. Exponential backoff + crash-loop detection:
   - configurable `initial_ms`, `factor`, `max_ms`
   - crash-loop window + threshold (e.g., 5 crashes in 60s → `blocked`)
   - deterministic schedule (no wall-clock jitter in tests)
3. Host tests proving:
   - state transitions are deterministic and bounded
   - backoff sequence matches schema (500ms → 1000ms → 2000ms... clamped)
   - crash-loop detection triggers `blocked` after threshold
   - kill reasons propagate correctly

## Non-Goals

- Kernel changes.
- OS/QEMU markers (deferred to v1.1b).
- Intent routing (reuses `intentsd` from `TASK-0126`).
- CPU budget enforcement (deferred to v1.1b; this task only defines the policy schema).

## Constraints / invariants (hard requirements)

- **No new lifecycle authority**: this extends `appmgrd` semantics, not a competing `abilityd`.
- **Determinism**: backoff schedules and state transitions must be stable given the same input sequence.
- **Bounded state**: crash history is bounded (ring buffer or TTL-based eviction).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (authority drift)**:
  - Do not introduce `abilityd` as a new service. Extend `appmgrd` (`TASK-0065`) or explicitly document consolidation/rename.
- **YELLOW (backoff determinism)**:
  - Backoff must use injectable time source in tests (not `std::time::SystemTime`).

## Contract sources (single source of truth)

- App lifecycle baseline: `TASK-0065`
- Execd spawn authority: `TASK-0001`
- Intent routing: `TASK-0126`

## Stop conditions (Definition of Done)

### Proof (Host) — required

`cargo test -p ability_lifecycle_v1_1_host` green (new):

- state machine: `stopped → starting → runningFg → stopping → stopped` is deterministic
- backoff: crash sequence produces `[500, 1000, 2000, 4000, 8000, 15000, 15000, ...]` (clamped)
- crash-loop: 5 crashes within 60s window → `blocked` state; manual unblock → `stopped`
- kill reasons: `crash`/`oom`/`policy`/`signal`/`request` propagate correctly in state updates

## Touched paths (allowlist)

- `userspace/libs/ability-lifecycle/` (new)
- `schemas/ability_v1_1.schema.json` (new)
- `tests/ability_v1_1_host/` (new)
- `docs/ability/overview.md` (new, host-first sections)

## Plan (small PRs)

1. **State machine + kill reasons**
   - define `AState` and `KillReason` enums
   - implement deterministic transition logic with bounded timeouts
   - host tests for happy path and crash path

2. **Backoff + crash-loop detection**
   - exponential backoff with injectable time source
   - crash history ring buffer (bounded)
   - crash-loop threshold logic
   - host tests for backoff sequence and block/unblock

3. **Schema + docs**
   - `schemas/ability_v1_1.schema.json` with backoff/crash-loop config
   - host-first docs

## Acceptance criteria (behavioral)

- State machine transitions are deterministic and bounded.
- Backoff schedule matches schema and is clamped correctly.
- Crash-loop detection triggers `blocked` deterministically after threshold.
- Kill reasons propagate correctly in state updates.
