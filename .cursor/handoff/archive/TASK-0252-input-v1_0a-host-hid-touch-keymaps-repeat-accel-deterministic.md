# Current Handoff: TASK-0252 closure complete (host input core)

**Date**: 2026-05-04  
**Completed task**: `tasks/TASK-0056B-ui-v2a-visible-input-cursor-focus-click.md` — `Done`  
**Completed contract**: `docs/rfcs/RFC-0051-ui-v2a-visible-input-cursor-focus-click-contract.md` — `Done`  
**Completed task**: `tasks/TASK-0252-input-v1_0a-host-hid-touch-keymaps-repeat-accel-deterministic.md` — `Done`  
**Completed contract seed**: `docs/rfcs/RFC-0052-input-v1_0a-host-hid-touch-keymaps-repeat-accel-contract.md` — `Done`  
**Next queue head**: `tasks/TASK-0253-input-v1_0b-os-hidrawd-touchd-inputd-ime-hooks-selftests.md` — `Draft`  
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

- `TASK-0252` status is now `Done` after host-proof + gate closure.
- `RFC-0052` contract seed exists and is linked from task + RFC index.
- Added explicit dependency on `TASK-0056B`.
- Added contract links for Gate-E quality mapping and authority naming.
- Added missing `Security / authority invariants` section.
- Expanded red flags with scope-drift risk and added explicit mitigation block.
- Extended stop conditions with required deterministic reject-path proofs.
- Corrected touched-paths drift:
  - from non-existing `userspace/libs/*` to `userspace/*` crate layout,
  - from non-existing `docs/input/overview.md` to `docs/dev/ui/input/input.md`.

## Completed execution focus for TASK-0252

- Implement host-first crates for HID parse, touch normalize, keymaps, key repeat, pointer accel.
- Add behavior-first tests as the primary proof surface:
  - positive deterministic behavior for EN/DE mapping, repeat timing, accel monotonicity/bounds, touch sequence ordering,
  - reject-path coverage for malformed reports and invalid repeat/accel configuration.
- Keep non-claims strict: no DTB, no OS/QEMU services, no `nx input` CLI in 0252 (belongs to 0253).
- Keep proof posture strict: Soll-behavior tests + `test_reject_*` are authoritative; no marker-based closure in 0252.

## 0252 implementation checkpoint (host core landed)

- Landed host-first crates:
  - `userspace/hid/` boot keyboard/mouse parser with stable reject classes,
  - `userspace/touch/` transport-neutral touch normalizer,
  - `userspace/keymaps/` shared base keymap authority for `us`, `de`, `jp`, `kr`, `zh`,
  - `userspace/key-repeat/` deterministic repeat scheduler over injectable monotonic time,
  - `userspace/pointer-accel/` bounded monotonic linear acceleration curve.
- Landed proof package:
  - `tests/input_v1_0_host/` with Soll vectors and `test_reject_*` suites for HID, touch, repeat, and accel.
- Green evidence recorded for the current slice:
  - `cargo test -p input_v1_0_host -- --nocapture`,
  - `just diag-host`,
  - `scripts/fmt-clippy-deny.sh`.
- Execution is `Done`:
  - docs/status sync is completed in task/RFC/index/workfiles,
  - broader repo gates (`just test-all`, `just ci-network`, `make clean/build/test/run`) were rerun on explicit user request and are green.

## Closure confirmation

- Host proof package for 0252 is green and deterministic.
- Task/RFC/docs sync preserves the 0252/0253 split and single-authority routing model.
- Gate-E quality claims remain behavior-backed (no marker-only closure language).
