# Current Handoff: TASK-0055C prep (visible SystemUI first frame)

**Date**: 2026-04-30  
**Active task**: `tasks/TASK-0055C-ui-v1d-windowd-visible-present-systemui-first-frame.md` — `In Progress`  
**Active contract**: `docs/rfcs/RFC-0049-ui-v1d-windowd-visible-present-systemui-first-frame-contract.md` — `In Progress`  
**Active contract carry-in**: `docs/rfcs/RFC-0048-ui-v1c-visible-qemu-scanout-bootstrap-contract.md` — `Done` (`TASK-0055B`)  
**Carry-in baseline**: `tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md` / `docs/rfcs/RFC-0047-ui-v1b-windowd-surface-layer-present-contract.md` — `Done`  
**Gate policy**: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate E: Windowing, UI & Graphics, `production-floor`)  

## Prep updates applied

- Archived the previous 55B closeout handoff to:
  - `.cursor/handoff/archive/TASK-0055B-ui-v1c-visible-qemu-scanout-bootstrap.md`
- Synced task tracking:
  - `TASK-0055B` added to the `Done` table in `tasks/IMPLEMENTATION-ORDER.md`.
- Hardened `TASK-0055C` task definition before execution:
  - Header now has real `depends-on` (`TASK-0055`, `TASK-0055B`) and explicit follow-up tasks (`TASK-0055D`, `TASK-0056`, `TASK-0056B`, `TASK-0056C`, `TASK-0251`).
  - Added `Security / authority invariants`.
  - Added explicit `Red flags / decision points` + mitigation.
  - Added Gate E alignment section mapped to `TRACK-PRODUCTION-GATES-KERNEL-SERVICES`.
  - Added host proof floor and closure gate list to stop conditions.
- Created and linked `RFC-0049` as the contract seed for `TASK-0055C`:
  - `docs/rfcs/RFC-0049-ui-v1d-windowd-visible-present-systemui-first-frame-contract.md`
  - linked from `TASK-0055C` header and `docs/rfcs/README.md`.

## Scope guardrails for 55C

- `TASK-0055C` proves visible `windowd` present + first visible SystemUI frame only.
- It does not claim input/cursor, dirty-rect perf closure, GPU/virtio-gpu, or kernel production-grade closure.
- Startup profile semantics stay separate from harness marker profiles.
