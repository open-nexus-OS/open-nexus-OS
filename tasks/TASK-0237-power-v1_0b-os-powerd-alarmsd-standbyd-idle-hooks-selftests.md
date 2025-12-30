---
title: TASK-0237 Power/Idle v1.0b (OS/QEMU): powerd + alarmsd + standbyd + kernel idle hooks + SystemUI battery/sleep + nx-power CLI + selftests
status: Draft
owner: @runtime
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Power core (host-first): tasks/TASK-0236-power-v1_0a-host-governor-wakelocks-residency-standby-deterministic.md
  - Timer coalescing baseline (timed): tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md
  - Ability lifecycle (BG detection): tasks/TASK-0235-ability-v1_1b-os-appmgrd-extension-samgr-hooks-fgbg-policies-selftests.md
  - Media sessions (wakelock hooks): tasks/TASK-0155-media-ux-v1a-host-mediasessd-focus-nowplaying-artcache.md
  - Settings v2 (provider keys): tasks/TASK-0225-settings-v2a-host-settingsd-typed-prefs-providers.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need OS/QEMU wiring for Power/Idle v1.0:

- `powerd` service integrating host-first governor + wake locks + residency,
- `alarmsd` service for RTC-like alarms (built atop `timed`),
- `standbyd` service for app standby enforcement,
- kernel idle hooks (minimal, simulated WFI/L2),
- SystemUI battery/sleep UI,
- `nx power` CLI.

The prompt proposes these services. This task delivers the **OS/QEMU integration** (services, kernel hooks, UI, CLI, selftests).

## Goal

On OS/QEMU:

1. `powerd` service:
   - integrates host-first governor + wake locks + residency
   - applies coalescing policy (delegated to `timed`)
   - sets kernel idle target (L1/L2 hints)
   - logs residency to `state:/metrics/power.jsonl` (if `/state` exists)
   - markers: `powerd: ready`, `powerd: mode=balanced`, `powerd: wakelock acquire kind=partial id=… app=…`, `powerd: residency l1=… l2=…`
2. Kernel idle hooks (minimal):
   - when run queue empty, emit `neuron: idle enter L1` and call `wait_for_interrupt()`
   - `powerd` can set target idle level (L1/L2) via small API
   - deep idle (L2) simulated by extending tick interval deterministically
   - markers: `neuron: idle target=L2`, `neuron: idle enter L1`, `neuron: idle leave L1`
3. `alarmsd` service:
   - RTC-like alarm queue built atop `timed`
   - integrates with `notifd` to raise `Priority::alarm` notifications
   - permission gate: `notifications` permission required; respects DND override
   - markers: `alarmsd: ready`, `alarmsd: fire id=…`
4. `standbyd` service:
   - watches `appmgrd` for BG apps
   - if BG, no active media, no foreground service for `bg_inactive_min` → standby
   - while standby: deny sensors/network (policy stubs), floor timers to `timer_floor_ms`
   - markers: `standbyd: ready`, `standbyd: enter app=…`, `standbyd: exit app=…`
5. SystemUI battery/sleep UI:
   - Battery tile (stubbed %)
   - Sleep now action in Quick Settings
   - Screen timeout & dim (read from schema)
   - Power graph (residency% over last 10 minutes)
   - markers: `ui: power tile open`, `ui: screen off`, `ui: power graph update`
   - Note: Full battery indicator (status icon, toasts, detail sheet) is handled by `TASK-0257` as an extension.
6. `nx power` CLI (subcommand of `nx`):
   - `status`, `mode`, `wakelock acquire/release`, `alarm`, `standby list/enter/exit`
7. Integrations:
   - `mediasessd`: active playback holds `partial` wakelock; releases on pause/stop
   - `settingsd`: provider keys `power.mode`, `power.screen.timeout_s`, `power.screen.dim_s` with side-effects to `powerd`
8. OS selftests + postflight.

## Non-Goals

- Real battery hardware (deterministic fixture only).
- Perfect energy model or advanced QoS policies.
- Real deep sleep (L2 is simulated via tick interval extension).

## Constraints / invariants (hard requirements)

- **No duplicate timer authority**: `alarmsd` and `standbyd` delegate to `timed` (`TASK-0013`), not a new timer service.
- **Kernel changes minimal**: idle hooks are notification-only (no scheduling changes).
- **Determinism**: idle residency sampling and wake lock state must be stable.
- **Bounded overhead**: residency logging is bounded (rolling window, not unbounded log).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (timer authority drift)**:
  - Do not introduce a new timer service. Reuse `timed` from `TASK-0013` for coalescing and alarm scheduling.
- **RED (missing lifecycle API)**:
  - If `appmgrd` cannot provide BG state notifications, this task must first create that API (separate subtask).
- **YELLOW (idle simulation)**:
  - L2 deep idle is simulated (extended tick interval), not real hardware deep sleep. Document this explicitly.

## Contract sources (single source of truth)

- QEMU marker contract: `scripts/qemu-test.sh`
- Power core: `TASK-0236`
- Timer coalescing: `TASK-0013`
- Ability lifecycle: `TASK-0235`

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — gated

UART markers:

- `powerd: ready`
- `powerd: mode=balanced`
- `powerd: wakelock acquire kind=partial id=… app=…`
- `powerd: residency l1=… l2=…`
- `alarmsd: ready`
- `alarmsd: fire id=…`
- `standbyd: ready`
- `standbyd: enter app=…`
- `neuron: idle target=L2`
- `neuron: idle enter L1`
- `SELFTEST: power wakelock+l2 ok`
- `SELFTEST: power alarm fire ok`
- `SELFTEST: power standby cycle ok`
- `SELFTEST: power residency ok`

## Touched paths (allowlist)

- `source/services/powerd/` (new)
- repo reality note: replace/rename `source/services/powermgr/` placeholder (see `tasks/TRACK-AUTHORITY-NAMING.md`)
- `source/services/alarmsd/` (new)
- `source/services/standbyd/` (new)
- `source/kernel/neuron/src/` (idle hooks, minimal)
- `source/services/timed/` (extend: alarm scheduling if needed)
- `source/services/mediasessd/` (extend: wakelock hooks)
- `source/services/settingsd/` (extend: power provider keys)
- SystemUI (battery tile + sleep controls + power graph)
- `tools/nx/` (extend: `nx power ...` subcommands)
- `source/apps/selftest-client/` (markers)
- `pkg://fixtures/power/` (battery curve fixture)
- `docs/power/timers.md` (new)
- `docs/tools/nx-power.md` (new)
- `tools/postflight-power-v1_0.sh` (new)

## Plan (small PRs)

1. **powerd service**
   - integrate host-first governor + wake locks + residency
   - kernel idle target API
   - markers

2. **Kernel idle hooks**
   - idle enter/leave notifications
   - target level hints (L1/L2)
   - L2 simulation (tick interval extension)

3. **alarmsd + standbyd**
   - alarmsd built atop timed
   - standbyd BG detection + standby enforcement
   - markers

4. **SystemUI + integrations + CLI + selftests**
   - battery tile + sleep controls
   - mediasessd wakelock hooks
   - settingsd provider keys
   - nx power CLI
   - OS selftests + postflight

## Acceptance criteria (behavioral)

- `powerd` manages governor modes and wake locks correctly.
- Kernel idle hooks emit correct markers.
- `alarmsd` fires alarms at scheduled times.
- `standbyd` enforces standby policies correctly.
- SystemUI battery/sleep UI works.
- All four OS selftest markers are emitted.
