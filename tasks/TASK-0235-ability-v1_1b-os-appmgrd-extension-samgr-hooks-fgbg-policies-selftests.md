---
title: TASK-0235 Ability/Lifecycle v1.1b (OS/QEMU): appmgrd extension + samgr hooks + FG/BG policies (CPU budget/timer slack) + kill reasons plumbing + nx-ability CLI + selftests
status: Draft
owner: @runtime
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Ability lifecycle core (host-first): tasks/TASK-0234-ability-v1_1a-host-lifecycle-state-backoff-killreasons-deterministic.md
  - App lifecycle baseline (appmgrd): tasks/TASK-0065-ui-v6b-app-lifecycle-notifications-navigation.md
  - Execd (spawner authority): tasks/TASK-0001-runtime-roles-and-boundaries.md
  - Intent routing (intentsd): tasks/TASK-0126-share-v2a-intentsd-registry-dispatch-policy-host.md
  - QoS/timer slack baseline: tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md
  - OOM watchdog (kill integration): tasks/TASK-0228-oomd-v1-deterministic-watchdog-cooperative-memstat-samgr-kill.md
  - Policy capability matrix (foreground guards): tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need OS/QEMU wiring for Ability/Lifecycle v1.1:

- extend `appmgrd` with the host-first lifecycle state machine,
- integrate with `samgr`/`execd` for spawn/exit notifications,
- enforce FG/BG policies (CPU budget, timer slack),
- plumb kill reasons end-to-end,
- provide `nx ability` CLI.

The prompt proposes a new `abilityd` service, but we already have `appmgrd` planned (`TASK-0065`). To avoid authority drift, this task **extends `appmgrd`** (not introducing a competing `abilityd`).

Repo reality note: the repo currently contains a placeholder `source/services/abilitymgr/`. The implementation must
**rename/replace** it to `appmgrd` rather than extending the placeholder name (see `tasks/TRACK-AUTHORITY-NAMING.md`).

## Goal

On OS/QEMU:

1. Extend `appmgrd` with lifecycle v1.1:
   - integrate host-first state machine (`TASK-0234`)
   - subscribe to `execd` exit notifications → update state with `KillReason`
   - apply backoff/crash-loop logic on abnormal exits
   - markers: `appmgrd: start app=… ab=…`, `appmgrd: crash app=… reason=…`, `appmgrd: backoff ms=…`, `appmgrd: blocked crash-loop`
2. samgr/execd hooks:
   - `samgr.spawn_with_profile(appId, exe, profile, env, fg:bool)` → returns `pid` and class
   - exit notifications → `appmgrd` with `(pid, appId, reason:KillReason, code/signal)`
   - kernel timer slack application (via QoS syscall if available)
3. FG/BG policy enforcement:
   - CPU budget sampler (reads proc runtime every 1s; accumulate in BG)
   - when `> cpu_budget_ms_per_min` → transition to `stopping` with `KillReason=policy`
   - timer slack: FG `5ms`, BG `200ms` (applied via kernel hint)
4. oomd integration:
   - `oomd` publishes OOM kills with `appId` → `appmgrd` updates state to `crashed` (`KillReason=oom`)
5. `nx ability` CLI (subcommand of `nx`):
   - `list`, `start`, `stop`, `fg`, `bg`, `send` (intent), `state`, `policy show|set`, `unblock`
6. OS selftests + postflight.

## Non-Goals

- Kernel changes (timer slack uses existing QoS syscall if available).
- Introducing `abilityd` as a new service (extends `appmgrd`).

## Constraints / invariants (hard requirements)

- **No new lifecycle authority**: extends `appmgrd`, not a competing `abilityd`.
- **Determinism**: CPU budget sampling and state transitions must be stable.
- **Bounded overhead**: CPU sampler uses bounded memory and rate-limited updates.
- **Single authority for kills**: `appmgrd` does not directly kill; it requests via `execd`/`samgr` or `oomd`.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (authority drift)**:
  - Do not introduce `abilityd` as a new service. Extend `appmgrd` (`TASK-0065`) or explicitly document consolidation/rename.
- **RED (missing lifecycle API)**:
  - If `execd`/`samgr` cannot provide exit notifications with kill reasons, this task must first create that API (separate subtask).
- **YELLOW (CPU budget truth)**:
  - CPU accounting via proc runtime sampling is not perfect; document this explicitly and ensure policy language does not claim otherwise.

## Contract sources (single source of truth)

- QEMU marker contract: `scripts/qemu-test.sh`
- Lifecycle core: `TASK-0234`
- Execd spawn authority: `TASK-0001`
- OOM watchdog: `TASK-0228`

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — gated

UART markers:

- `appmgrd: ready`
- `appmgrd: start app=… ab=…`
- `appmgrd: fg app=…` / `appmgrd: bg app=…`
- `appmgrd: crash app=… reason=crash`
- `appmgrd: backoff ms=…`
- `appmgrd: blocked crash-loop`
- `appmgrd: bg budget exceeded app=…`
- `SELFTEST: ability start fg ok`
- `SELFTEST: ability bg budget ok`
- `SELFTEST: ability backoff+unblock ok`
- `SELFTEST: ability intent ok`

## Touched paths (allowlist)

- `source/services/appmgrd/` (extend: lifecycle v1.1 + FG/BG policies; rename/replace `source/services/abilitymgr/` placeholder)
- `source/services/samgrd/` (extend: spawn_with_profile + exit notifications)
- `source/services/execd/` (extend: exit notifications with kill reasons)
- `tools/nx/` (extend: `nx ability ...` subcommands)
- `source/apps/selftest-client/` (markers)
- `userspace/apps/demo-ability/` (new: page/service demo with crash/busy flags)
- `docs/ability/policy.md` (new)
- `docs/tools/nx-ability.md` (new)
- `tools/postflight-ability-v1_1.sh` (new)

## Plan (small PRs)

1. **appmgrd lifecycle extension**
   - integrate host-first state machine
   - subscribe to execd exit notifications
   - apply backoff/crash-loop logic
   - markers

2. **samgr/execd hooks**
   - `spawn_with_profile` API
   - exit notifications with kill reasons
   - timer slack application

3. **FG/BG policy enforcement**
   - CPU budget sampler
   - policy stop on budget exceed
   - oomd integration

4. **nx ability CLI + demo + selftests**
   - CLI subcommands
   - demo-ability app
   - OS selftests + postflight

## Acceptance criteria (behavioral)

- `appmgrd` manages lifecycle states deterministically.
- FG/BG transitions apply timer slack correctly.
- CPU budget enforcement stops BG processes that exceed budget.
- Crash/backoff/crash-loop detection works end-to-end.
- Kill reasons propagate correctly from execd/oomd to appmgrd state.
