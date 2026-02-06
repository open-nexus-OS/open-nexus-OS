---
title: TASK-0259 Sensor-Bus v0.9b (OS/QEMU): i2cd/spid bus services + sensord aggregator + permissions/indicators + `nx sensor` + selftests
status: Draft
owner: @runtime
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Authority & naming registry: tasks/TRACK-AUTHORITY-NAMING.md
  - Sensor core (host-first): tasks/TASK-0258-sensor-bus-v0_9a-host-sensor-hal-accel-driver-deterministic.md
  - Device MMIO access: tasks/TASK-0010-device-mmio-access-model.md
  - Privacy dashboard: tasks/TASK-0168-policy-v1_1-os-runtime-prompts-privacy-dashboard-cli.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need OS/QEMU integration for Sensor-Bus v0.9:

- `i2cd`/`spid` bus services,
- `sensord` aggregator (subscriptions/rates/flush),
- permissions & indicators,
- `nx sensor` CLI.

The prompt proposes these services. This task delivers the **OS/QEMU integration** with bus services, sensor aggregator, permissions/indicators, and CLI, complementing the host-first sensor HAL and accelerometer driver.

## Goal

On OS/QEMU:

1. **DTB updates**:
   - extend `pkg://dts/virt-nexus.dts`:
     - `i2c@10004000` (status = "okay") with clock/irq stubs
     - `accelerometer@18` on that bus, `compatible = "nexus,lis3dh-null"`, properties: `range = <2>` (±2g), `odr_hz = <50>` (default output data rate), `int-gpio = <&gpio0 5 0>` (stubbed)
   - rebuild DTB
2. **Bus services** (`source/services/i2cd/`, `source/services/spid/`):
   - `i2cd` service:
     - backend switch: `hw` (MMIO stub, placeholder), `sim` (deterministic virtual device table populated from DT)
     - API (`i2c.capnp`): `xfer(m:Msg) -> data`, `scan() -> addrs:List(UInt8)`
     - rate limit logs; markers: `i2cd: ready`, `i2cd: scan addrs=[0x18]`
   - `spid` service:
     - same pattern but leave **sim only** for now
     - markers: `spid: ready`
3. **sensord aggregator** (`source/services/sensord/`):
   - owns the driver instance; run a deterministic tick loop at requested rate(s)
   - downsample/upsample clients; **onChange** threshold baked (e.g., 0.05 m/s²)
   - enforce **permissions** (see below)
   - API (`sensor.capnp`): `info()` → `AccelInfo`, `subscribe(rate)` → `stream:List(AccelSample)`, `setRate(rate)`, `flush()`
   - markers: `sensord: ready`, `sensord: rate=50Hz clients=1`, `sensord: flush delivered=…`
4. **Permissions & Indicators**:
   - **permissionsd**: add `sensors.accel` permission (scopes: `whileInUse` or `allow`)
   - **SystemUI indicator**: small status dot when any app subscribes; hides after all unsubscribed (debounced)
   - **Privacy log**: append entries to `state:/privacy/sensors.ndjson` with appId, start/stop, rate (gated on `/state`)
   - markers: `permissions: sensors.accel allow`, `ui: sensor indicator on/off`
5. **CLI diagnostics** (`nx sensor ...` as a subcommand of the canonical `nx` tool; see `tasks/TRACK-AUTHORITY-NAMING.md`):
   - `nx sensor info accel`, `nx sensor stream accel --rate 50 --dur 3s`, `nx sensor flush accel`, `nx sensor fixture accel on|off`
   - markers: `nx: sensor info accel`, `nx: sensor stream n=…`
6. **Settings integration**:
   - seed keys: `sensors.accel.rate` (default 50), `sensors.accel.fixture` (bool)
   - provider side-effects: update `sensord` rate and driver fixture mode
7. **OS selftests + postflight**.

## Non-Goals

- Kernel changes.
- Real hardware (QEMU/null accelerometer only).
- Full sensor suite (only accelerometer for v0.9).

## Constraints / invariants (hard requirements)

- **No duplicate sensor authority**: `sensord` is the single authority for sensor subscriptions and rate control. Do not create parallel sensor services.
- **No duplicate bus authority**: `i2cd`/`spid` are the single authorities for I²C/SPI bus access. Do not create parallel bus services.
- **Determinism**: bus services, sensor aggregator, permissions, and indicators must be stable given the same inputs.
- **Bounded resources**: sensor subscriptions are bounded; privacy log is size-bounded.
- **Device access**: real hardware I²C/SPI drivers assume `TASK-0010` (device MMIO access model) is Done. For v0.9, the
  sim backend is sufficient.
- **Persistence gating**: privacy log requires `/state` (`TASK-0009`) or equivalent. Without `/state`, privacy log must be disabled or explicit `stub/placeholder` (no "written ok" claims).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (sensor authority drift)**:
  - Do not create a parallel sensor service that conflicts with `sensord`. `sensord` is the single authority for sensor subscriptions and rate control.
- **RED (bus authority drift)**:
  - Do not create parallel bus services. `i2cd`/`spid` are the single authorities for I²C/SPI bus access.
- **YELLOW (permissions integration)**:
  - `sensors.accel` permission must integrate with existing permissionsd (`TASK-0168`). Document the relationship explicitly.

## Contract sources (single source of truth)

- QEMU marker contract: `scripts/qemu-test.sh`
- Sensor core: `TASK-0258`
- Device MMIO access: `TASK-0010` (prerequisite for real hardware drivers)
- Privacy dashboard: `TASK-0168` (permissions integration)
- Persistence: `TASK-0009` (prerequisite for `/state`)

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — gated

UART markers:

- `i2cd: ready`
- `i2cd: scan addrs=[0x18]`
- `spid: ready`
- `sensord: ready`
- `sensord: rate=50Hz clients=1`
- `sensord: flush delivered=…`
- `permissions: sensors.accel allow`
- `ui: sensor indicator on/off`
- `SELFTEST: sensor accel 50Hz ok`
- `SELFTEST: sensor accel onChange ok`
- `SELFTEST: sensor fixture ok`
- `SELFTEST: sensor indicator ok`

## Touched paths (allowlist)

- `pkg://dts/virt-nexus.dts` (extend: I²C node + accelerometer)
- `source/services/i2cd/` (new)
- `source/services/spid/` (new)
- `source/services/sensord/` (new)
- `source/services/permissionsd/` (extend: sensors.accel permission)
- SystemUI (sensor indicator)
- `source/services/settingsd/` (extend: sensors provider keys)
- `tools/nx/` (extend: `nx sensor ...` subcommands; no separate `nx-sensor` binary)
- `source/apps/selftest-client/` (markers)
- `docs/sensors/overview.md` (new)
- `docs/sensors/accel_lis3dh.md` (new)
- `docs/tools/nx-sensor.md` (new)
- `docs/privacy/overview.md` (extend: sensors permission & indicator)
- `tools/postflight-sensor-bus-v0_9.sh` (new)

## Plan (small PRs)

1. **DTB + bus services**
   - DTB: I²C node + accelerometer
   - i2cd/spid services (sim backend)
   - markers

2. **sensord aggregator**
   - sensord service (subscriptions/rates/flush)
   - permissions enforcement
   - markers

3. **Permissions + indicators + CLI + selftests**
   - permissionsd integration
   - SystemUI indicator
   - privacy log (gated on `/state`)
   - `nx sensor` CLI
   - settings provider
   - OS selftests + postflight

## Acceptance criteria (behavioral)

- `i2cd`/`spid` scan devices correctly.
- `sensord` streams samples at requested rates; onChange threshold works correctly.
- Permissions enforcement works correctly.
- SystemUI indicator toggles on subscribe/unsubscribe.
- All four OS selftest markers are emitted.
