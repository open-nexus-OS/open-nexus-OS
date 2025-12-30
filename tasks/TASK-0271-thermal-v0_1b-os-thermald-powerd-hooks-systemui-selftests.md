---
title: TASK-0271 Thermal v0.1b (OS/QEMU): thermald service + powerd hooks + SystemUI indicators + selftests
status: Draft
owner: @platform
created: 2025-12-30
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Authority & naming registry: tasks/TRACK-AUTHORITY-NAMING.md
  - Power/Idle: tasks/TASK-0237-power-v1_0b-os-powerd-alarmsd-standbyd-idle-hooks-selftests.md
  - Battery: tasks/TASK-0257-battery-v0_9b-os-batteryd-powerd-hooks-systemui-selftests.md
  - Device MMIO model (future real sensors): tasks/TASK-0010-device-mmio-access-model.md
  - Persistence: tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Repo reality includes `source/services/thermalmgr/` as a placeholder. The canonical authority is `thermald`
(see `TRACK-AUTHORITY-NAMING.md`). Thermal is part of “modern + safe” (avoid overheating/throttle policy drift),
but must remain deterministic and QEMU-proof.

## Goal

On OS/QEMU:

1. **Define `thermald` authority** (`source/services/thermald/`):
   - single place that tracks thermal zones and state.
   - exposes a small IPC surface: `Thermal.status()`, `Thermal.subscribe()`, `Thermal.inject()` (test-only).
2. **Deterministic model (QEMU-first)**:
   - default: fixed ambient 25°C, zones stable unless injected.
   - deterministic throttling thresholds from schema (warn/hot/critical).
3. **Integrate with `powerd`**:
   - `thermald` can request `powerd` to enter a reduced mode (e.g. `frugal`) when hot/critical.
   - `powerd` remains the authority for governor/wakelocks; `thermald` is authority for thermal sensing/policy inputs.
4. **SystemUI**:
   - indicator/toast when entering hot/critical state.
   - markers: `ui: thermal toast hot`, `ui: thermal toast critical`.
5. **Proof markers (QEMU)**:
   - `thermald: ready`
   - `thermald: zone cpu temp=... state=...`
   - `thermald: request power mode=frugal reason=thermal_hot`

## Non-Goals

- Real hardware thermal sensors in v0.1 (future: may use I²C/SPI or DT-based zones; gated on `TASK-0010`).
- Kernel thermal framework.
- Background daemon sprawl (no parallel `thermalmgr`).

## Constraints / invariants (hard requirements)

- **Single thermal authority**: `thermald` only. `thermalmgr` placeholder must be replaced/removed.
- **Determinism**: state transitions are input-driven (schema + inject), not timer-flaky.
- **No fake success**: UI “hot” indicators only when `thermald` state is actually hot/critical.
- **Bounded**: bounded zone count, bounded subscriber fanout.
- **Persistence gating (optional)**: if we persist last known state, it must be gated on `/state` (`TASK-0009`).

## Touched paths (allowlist)

- `source/services/thermald/**` (new; replace/remove `thermalmgr` placeholder)
- `source/services/powerd/**` (hooks only, no new authority)
- `userspace/apps/systemui/**`
- `schemas/**`
- `scripts/qemu-test.sh` (markers)
