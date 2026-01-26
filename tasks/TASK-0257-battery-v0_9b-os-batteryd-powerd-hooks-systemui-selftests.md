---
title: TASK-0257 Battery v0.9b (OS/QEMU): batteryd service + powerd hooks + SystemUI battery indicator + `nx battery` + selftests
status: Draft
owner: @runtime
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Authority & naming registry: tasks/TRACK-AUTHORITY-NAMING.md
  - Battery core (host-first): tasks/TASK-0256-battery-v0_9a-host-fuel-gauge-model-charger-deterministic.md
  - Power/idle baseline: tasks/TASK-0236-power-v1_0a-host-governor-wakelocks-residency-standby-deterministic.md
  - Power/idle OS: tasks/TASK-0237-power-v1_0b-os-powerd-alarmsd-standbyd-idle-hooks-selftests.md
  - Device MMIO access: tasks/TASK-0010-device-mmio-access-model.md
  - Testing contract: scripts/qemu-test.sh
  - Data formats rubric (JSON vs Cap'n Proto): docs/adr/0021-structured-data-formats-json-vs-capnp.md
---

## Context

We need OS/QEMU integration for Battery v0.9:

- `batteryd` service (fuel-gauge model + charger state),
- `powerd` hooks (low/critical actions),
- SystemUI battery indicator (status icon, toasts, detail sheet).

The prompt proposes these services. `TASK-0236`/`TASK-0237` already plan `powerd` (governor, wake-locks, residency). This task delivers the **OS/QEMU integration** with `batteryd` service and `powerd` hooks for low/critical actions, complementing the existing power management system.

## Goal

On OS/QEMU:

1. **DTB updates**:
   - extend `pkg://dts/virt-nexus.dts`:
     - node `fuel-gauge@i2c1` (compatible `"nexus,fuelgauge-null"`), properties: `r_sense_milliohm`, `voltage-{full,empty,nom}`
     - node `charger@i2c1` (compatible `"nexus,charger-null"`) with `max_current_ma`, `plug-detect` GPIO (stub)
   - rebuild DTB
2. **Schema & Policy**:
   - add `schemas/battery_v0_9.schema.json` (sample_period_ms, model, thresholds, temp_limits_c, cycle_estimate_mwh)
   - policyd caps: `battery.status.read`, `battery.calibrate`, `battery.inject` (test only), `power.lowpower.enter`, `power.shutdown.request`
   - marker: `policy: battery v0.9 caps enforced`
3. **batteryd service** (`source/services/batteryd/`; repo reality: replace/rename `source/services/batterymgr/` placeholder):
   - deterministic model: simple Coulomb counter with OCV clamp between `voltage_empty_mv..voltage_full_mv`; temperature stable 25 °C unless injected (using library from `TASK-0256`)
   - charger: when `plug=true`, state `charging` until 100%, then `full`; unplug sets `discharging` (using library from `TASK-0256`)
   - persistence: store cycles and last SoC under `state:/battery/state.nxs` (Cap'n Proto snapshot; canonical; gated on `/state`)
     - optional derived/debug view: `nx battery export --json` emits deterministic JSON
   - publish to `powerd` on each sample; trigger threshold events (low/critical/thermal hot/cold) (using library from `TASK-0256`)
   - API (`battery.capnp`): `get()` → `Sample`, `subscribe()` → `samples:List(Sample)`, `calibrate(mwh)`, `inject(deltaPct, plug)` (test-only deterministic)
   - markers: `batteryd: ready`, `batteryd: sample pct=.. mv=.. ma=.. state=charging`, `batteryd: threshold low`, `batteryd: threshold critical`
4. **powerd integration** (extend `powerd` from `TASK-0237`):
   - on `low` (≤ `low_pct`): set mode `frugal`, coalesce timers to frugal value, post SystemUI toast
   - on `critical` (≤ `critical_pct`): request **screen off**, schedule **auto-sleep** after 30 s, and if SoC still dropping at next two samples → call graceful **shutdown**
   - on `plugged in`: restore previous mode
   - markers: `powerd: battery low → frugal`, `powerd: battery critical → sleep`, `powerd: battery shutdown requested`
5. **SystemUI battery indicator & detail**:
   - status icon (charging bolt/plug/full/unplugged) with percentage text
   - toast on low/critical
   - detail sheet shows: SoC%, state, voltage, current, temperature, cycles; last 30 min sparkline (residency-style reuse); actions: "Battery saver" (toggle frugal), "Screen off now"
   - subscribe to `l10nd` for localized strings
   - markers: `ui: battery toast low`, `ui: battery sheet open`
6. **CLI diagnostics** (`nx battery ...` as a subcommand of the canonical `nx` tool; see `tasks/TRACK-AUTHORITY-NAMING.md`):
   - `nx battery status`, `nx battery inject --delta -5` (test-only: drop 5%), `nx battery plug on|off`, `nx battery calibrate --mwh 18000`, `nx battery saver on|off` (proxies to powerd mode)
   - markers: `nx: battery status pct=..`, `nx: battery inject -5`, `nx: battery plug on`
7. **Settings integration**:
   - seed keys in `settingsd`: `battery.saver.auto` (bool, default true) — auto-enter frugal on low, `battery.toast.every_pct` (int, default from schema)
   - provider wires into `powerd` and `batteryd`
8. **OS selftests + postflight**.

## Non-Goals

- Kernel changes.
- Real hardware (QEMU/null fuel-gauge/charger only).
- Full power management (handled by `TASK-0237`).

## Constraints / invariants (hard requirements)

- **No duplicate battery authority**: `batteryd` is the single authority for battery state. Do not create parallel battery services.
- **No duplicate power authority**: `powerd` is the single authority for power management. This task extends `powerd` with battery hooks, not a new power service.
- **Determinism**: fuel-gauge model, charger state, thresholds, and persistence must be stable given the same inputs.
- **Bounded resources**: battery state is bounded; cycles counter is bounded.
- **Device MMIO gating**: userspace fuel-gauge/charger drivers may require `TASK-0010` (device MMIO access model) or equivalent.
- **Persistence gating**: persistence requires `/state` (`TASK-0009`) or equivalent. Without `/state`, persistence must be disabled or explicit `stub/placeholder` (no "written ok" claims).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (battery authority drift)**:
  - Do not create a parallel battery service that conflicts with `batteryd`. `batteryd` is the single authority for battery state.
- **RED (power authority drift)**:
  - Do not create a parallel power service. This task extends `powerd` from `TASK-0237` with battery hooks, not a new power service.
- **YELLOW (powerd extension)**:
  - `TASK-0237` already plans `powerd` service. This task extends it with battery hooks (low/critical actions). Document the relationship explicitly: `batteryd` publishes samples → `powerd` reacts to thresholds.

## Contract sources (single source of truth)

- QEMU marker contract: `scripts/qemu-test.sh`
- Battery core: `TASK-0256`
- Power/idle baseline: `TASK-0236`/`TASK-0237` (powerd service)
- Device MMIO access: `TASK-0010` (prerequisite for fuel-gauge/charger)
- Persistence: `TASK-0009` (prerequisite for `/state`)

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — gated

UART markers:

- `policy: battery v0.9 caps enforced`
- `batteryd: ready`
- `batteryd: sample pct=.. mv=.. ma=.. state=charging`
- `batteryd: threshold low`
- `batteryd: threshold critical`
- `powerd: battery low → frugal`
- `powerd: battery critical → sleep`
- `powerd: battery shutdown requested`
- `ui: battery toast low`
- `ui: battery sheet open`
- `SELFTEST: battery low ok`
- `SELFTEST: battery critical ok`
- `SELFTEST: battery charge ok`
- `SELFTEST: battery sheet ok`

## Touched paths (allowlist)

- `pkg://dts/virt-nexus.dts` (extend: fuel-gauge + charger null nodes)
- `schemas/battery_v0_9.schema.json` (new)
- `source/services/batteryd/` (new; canonical authority, see `tasks/TRACK-AUTHORITY-NAMING.md`)
- `source/services/powerd/` (extend: battery hooks from `TASK-0237`)
- `source/services/policyd/` (extend: battery caps)
- SystemUI (battery indicator + toasts + detail sheet)
- `source/services/settingsd/` (extend: battery provider keys)
- `tools/nx/` (extend: `nx battery ...` subcommands; no separate `nx-battery` binary)
- `source/apps/selftest-client/` (markers)
- `docs/power/battery_v0_9.md` (new)
- `docs/tools/nx-battery.md` (new)
- `tools/postflight-battery-v0_9.sh` (new)

## Plan (small PRs)

1. **DTB + schema + policy + batteryd service**
   - DTB: fuel-gauge + charger nodes
   - schema + policy caps
   - batteryd service (using libraries from `TASK-0256`)
   - markers

2. **powerd hooks + SystemUI**
   - powerd battery hooks (low/critical actions)
   - SystemUI battery indicator + toasts + detail sheet
   - markers

3. **CLI + settings + selftests**
   - nx-battery CLI
   - settings provider
   - OS selftests + postflight

## Acceptance criteria (behavioral)

- `batteryd` publishes samples and triggers threshold events correctly.
- `powerd` reacts to low/critical thresholds (frugal/sleep/shutdown) correctly.
- SystemUI battery indicator, toasts, and detail sheet work correctly.
- All four OS selftest markers are emitted.
