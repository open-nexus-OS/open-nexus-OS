---
title: TASK-0258 Sensor-Bus v0.9a (host-first): sensor HAL + accelerometer driver + deterministic tests
status: Draft
owner: @runtime
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Device MMIO access: tasks/TASK-0010-device-mmio-access-model.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need a deterministic sensor stack foundation:

- sensor HAL (common types, traits, helpers),
- accelerometer driver (LIS3DH-like),
- scaling/units (raw counts → m/s²),
- fixture mode (deterministic samples).

The prompt proposes a sensor HAL and accelerometer driver. This task delivers the **host-first core** (sensor HAL, accelerometer driver, scaling/units, fixture mode) that can be reused by both OS/QEMU integration and host tests.

## Goal

Deliver on host:

1. **Sensor HAL library** (`userspace/libs/sensor_hal/`):
   - common types: `SensorType` (Accel, later Gyro, Mag, …), `Sample3 { ts_ns, x, y, z }` (m/s²), `Info { sensor, name, vendor, range_g, odr_hz }`
   - trait: `SensorDriver { info(), configure(odr_hz, range_g), poll() -> Option<Sample3> }`
   - helpers: fixed-point conversions, LIS3DH register map, scale to m/s² (`1g = 9.80665`)
2. **Accelerometer driver** (`userspace/drivers/sensor_accel_lis3dh/`):
   - uses I²C sim backend (virtual device table populated from DT)
   - supported ranges: ±2/4/8/16 g; ODR: {10, 25, 50, 100} Hz (rounded)
   - returns deterministic samples from a seeded PRNG **or** from a fixture file when `SENSOR_FIXTURE=1`
3. **Host tests** proving:
   - scale/units: raw counts → m/s² exact for ±2g at 1 LSB = 0.001 g (verify math)
   - rate control: subscription at 25 Hz yields ~75 samples in 3 s with tolerance ±1
   - onChange: constant vector below threshold emits ≤ 1 sample; step above threshold emits within 1 tick
   - fixture mode: fixed CSV fixture produces stable hash over first N samples

## Non-Goals

- OS/QEMU integration (deferred to v0.9b).
- Real hardware (QEMU/null accelerometer only).
- Full sensor suite (only accelerometer for v0.9).

## Constraints / invariants (hard requirements)

- **No duplicate sensor authority**: This task provides sensor HAL library. OS/QEMU integration (`TASK-0259`) should use the same HAL to avoid drift.
- **Determinism**: sensor HAL, accelerometer driver, scaling/units, and fixture mode must be stable given the same inputs.
- **Bounded resources**: sensor samples are bounded; fixture files are size-bounded.
- **Device access note**: the I²C sim backend does not require real MMIO. Real hardware drivers assume `TASK-0010`
  (device MMIO access model) is Done and may require additional device-class caps.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (sensor authority drift)**:
  - Do not create parallel sensor HAL contracts. This task provides sensor HAL library. OS/QEMU integration (`TASK-0259`) should use the same HAL to avoid drift.
- **YELLOW (scaling determinism)**:
  - Scaling math must use fixed-point or deterministic floating-point (stable rounding rules).

## Contract sources (single source of truth)

- Testing contract: `scripts/qemu-test.sh`
- Device MMIO access: `TASK-0010` (prerequisite for real hardware drivers)

## Stop conditions (Definition of Done)

### Proof (Host) — required

`cargo test -p sensor_bus_v0_9_host` green (new):

- scale/units: raw counts → m/s² exact for ±2g at 1 LSB = 0.001 g (verify math)
- rate control: subscription at 25 Hz yields ~75 samples in 3 s with tolerance ±1
- onChange: constant vector below threshold emits ≤ 1 sample; step above threshold emits within 1 tick
- fixture mode: fixed CSV fixture produces stable hash over first N samples

## Touched paths (allowlist)

- `userspace/libs/sensor_hal/` (new)
- `userspace/drivers/sensor_accel_lis3dh/` (new)
- `pkg://fixtures/sensors/` (new; CSV fixtures)
- `tests/sensor_bus_v0_9_host/` (new)
- `docs/sensors/overview.md` (new, host-first sections)
- `docs/sensors/accel_lis3dh.md` (new)

## Plan (small PRs)

1. **Sensor HAL**
   - common types + trait + helpers
   - host tests

2. **Accelerometer driver**
   - LIS3DH-like driver (sim backend)
   - fixture mode
   - host tests

3. **Docs**
   - host-first docs

## Acceptance criteria (behavioral)

- Sensor HAL provides stable types and trait.
- Accelerometer driver scales raw counts to m/s² correctly.
- Rate control works correctly (25 Hz yields ~75 samples in 3 s).
- onChange threshold works correctly.
- Fixture mode produces stable hash.
