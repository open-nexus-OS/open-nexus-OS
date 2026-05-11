# Current Handoff: TASK-0056C kickoff after TASK-0253 done closeout

**Date**: 2026-05-11  
**Active task**: `tasks/TASK-0056C-ui-v2a-present-input-perf-latency-coalescing.md` — `Draft`  
**Contract seed**: `docs/rfcs/RFC-0055-ui-v2a-embedded-reactor-runtime-floor-present-input-perf-contract.md` — `Draft`  
**Carry-in completed task**: `tasks/TASK-0253-input-v1_0b-os-hidrawd-touchd-inputd-ime-hooks-selftests.md` — `Done`  
**Carry-in RFCs**: `docs/rfcs/RFC-0053-input-v1_0b-os-qemu-live-input-hidrawd-touchd-inputd-contract.md` / `docs/rfcs/RFC-0054-input-v1_0c-os-qemu-virtio-input-driver-layer-contract.md` — `Done`  
**Gate policy**: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate E: Windowing, UI & Graphics, `production-floor`)  

## Starting point

- The live service-owned input chain is now the real carry-in:
  - `virtio-input -> hidrawd -> inputd -> windowd -> fbdevd -> ramfb`
  - `selftest-client` remains observer-only for visible/input proof collection.
- Existing code already exposes useful 56C seams:
  - `inputd` chain counters and idle-yield telemetry,
  - `windowd` compose/present/coalesced/damage telemetry plus present-fence latency data,
  - `fbdevd` cadence/flush/scanout telemetry and a bounded present reactor.
- What remains open for 56C:
  - deterministic pointer-motion coalescing rules,
  - no-damage / no-visible-state-change skip rules,
  - honest idle-cheap behavior,
  - a task-owned latency/coalescing proof package and marker ladder.

## Guardrails

- Keep one embedded runtime/reactor floor; do not build a detached subsystem beside `inputd`, `windowd`, or `fbdevd`.
- Keep authority boundaries explicit:
  - `inputd` = normalized input authority,
  - `windowd` = hit-test/focus/click/compose authority,
  - `fbdevd` = cadence + scanout authority.
- Pointer-motion bursts may coalesce; click/focus-transfer/wheel/key edges must remain explicit and individually observable.
- Perf markers may fire only after a real visible update or an explicit proven no-damage/no-visible-change decision.
- Do not back-claim any perf closure from `TASK-0253`; 0253 stays the live-input carry-in only.

## First proof targets

- Host proof package or equivalent requirement-named suites for:
  - deterministic motion-burst coalescing,
  - forbidden semantic-edge collapse rejects,
  - no-damage skip honesty,
  - idle-cheap boundedness.
- QEMU marker ladder for:
  - `windowd: present fastpath on`
  - `windowd: pointer coalesce ok`
  - `windowd: no-damage skip ok`
  - `windowd: idle fastpath ok`
  - `windowd: click latency ok`
  - `windowd: keyboard latency ok`
  - `SELFTEST: live pointer latency ok`
  - `SELFTEST: live keyboard latency ok`
  - `SELFTEST: ui v2 perf ok`

## Out of scope in this slice

- Scroll/clip/effects/IME breadth (`TASK-0059`).
- Runtime/animation and invalidation breadth (`TASK-0062`, `TASK-0063`).
- WM/scene-transition breadth (`TASK-0064`).
- Kernel perf-floor redesign in `TASK-0054B` / `TASK-0054C` / `TASK-0054D`.
