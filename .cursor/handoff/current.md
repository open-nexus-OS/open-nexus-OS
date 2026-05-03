# Current Handoff: TASK-0252 active execution checkpoint (host input core)

**Date**: 2026-05-03  
**Completed task**: `tasks/TASK-0056B-ui-v2a-visible-input-cursor-focus-click.md` — `Done`  
**Completed contract**: `docs/rfcs/RFC-0051-ui-v2a-visible-input-cursor-focus-click-contract.md` — `Done`  
**Active task**: `tasks/TASK-0252-input-v1_0a-host-hid-touch-keymaps-repeat-accel-deterministic.md` — `In Progress`  
**Active contract seed**: `docs/rfcs/RFC-0052-input-v1_0a-host-hid-touch-keymaps-repeat-accel-contract.md` — `In Progress`  
**Gate policy**: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate E: Windowing, UI & Graphics, `production-floor`)  

## Carry-in baseline (must stay true)

- `TASK-0055`/`TASK-0055B`/`TASK-0055C`/`TASK-0056`/`TASK-0056B` are `Done` and form the UI carry-in.
- Live input architecture is split intentionally:
  - `TASK-0252`: host-first input core libraries (hid/touch/keymaps/repeat/accel),
  - `TASK-0253`: OS/QEMU services (`hidrawd`/`touchd`/`inputd`) and live device path.
- Input authority boundaries remain explicit:
  - `windowd` owns hit-test/hover/focus/click authority,
  - 0252 provides deterministic event-core primitives only.

## 0252 prep and contract hardening (completed)

- `TASK-0252` status moved to `In Progress` to match queue-head reality.
- `RFC-0052` contract seed exists and is linked from task + RFC index.
- Added explicit dependency on `TASK-0056B`.
- Added contract links for Gate-E quality mapping and authority naming.
- Added missing `Security / authority invariants` section.
- Expanded red flags with scope-drift risk and added explicit mitigation block.
- Extended stop conditions with required deterministic reject-path proofs.
- Corrected touched-paths drift:
  - from non-existing `userspace/libs/*` to `userspace/*` crate layout,
  - from non-existing `docs/input/overview.md` to `docs/dev/ui/input/input.md`.

## Immediate execution focus for TASK-0252

- Implement host-first crates for HID parse, touch normalize, keymaps, key repeat, pointer accel.
- Add behavior-first tests as the primary proof surface:
  - positive deterministic behavior for EN/DE mapping, repeat timing, accel monotonicity/bounds, touch sequence ordering,
  - reject-path coverage for malformed reports and invalid repeat/accel configuration.
- Keep non-claims strict: no DTB, no OS/QEMU services, no `nx input` CLI in 0252 (belongs to 0253).
- Keep proof posture strict: Soll-behavior tests + `test_reject_*` are authoritative; no marker-based closure in 0252.

## Closure prerequisites before 0252 Done

- Host proof package for 0252 must be green and deterministic.
- Task/RFC/docs sync must preserve the 0252/0253 split and single-authority routing model.
- Gate-E quality claims must remain behavior-backed (no marker-only closure language).
