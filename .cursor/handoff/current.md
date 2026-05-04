# Current Handoff: TASK-0253 prep checkpoint (OS/QEMU live input path)

**Date**: 2026-05-04  
**Completed task**: `tasks/TASK-0252-input-v1_0a-host-hid-touch-keymaps-repeat-accel-deterministic.md` — `Done`  
**Completed contract seed**: `docs/rfcs/RFC-0052-input-v1_0a-host-hid-touch-keymaps-repeat-accel-contract.md` — `Done`  
**Active task**: `tasks/TASK-0253-input-v1_0b-os-hidrawd-touchd-inputd-ime-hooks-selftests.md` — `In Progress`  
**Active contract seed**: `docs/rfcs/RFC-0053-input-v1_0b-os-qemu-live-input-hidrawd-touchd-inputd-contract.md` — `In Progress`  
**Gate policy**: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate E: Windowing, UI & Graphics, `production-floor`)  

## Carry-in baseline (must stay true)

- `TASK-0055`/`TASK-0055B`/`TASK-0055C`/`TASK-0056`/`TASK-0056B` are `Done` and remain the visible-input carry-in.
- `TASK-0252` host-first core crates/tests are `Done` and are the only parser/keymap/repeat/accel authority.
- Live input architecture split remains explicit:
  - `TASK-0252`: host core behavior authority,
  - `TASK-0253`: OS/QEMU ingestion/services/routing into existing UI authority.
- Input authority boundaries remain explicit:
  - `inputd` normalizes/routes low-level input,
  - `windowd` remains hit-test/hover/focus/click authority,
  - IME full behavior remains follow-up scope (`TASK-0146`/`TASK-0147`).

## TASK-0253 prep hardening (completed in this slice)

- `TASK-0253` status moved to `In Progress` for active queue reality.
- RFC contract seed is now present and linked (`RFC-0053`).
- Header follow-ups are populated: `TASK-0056C`, `TASK-0146`, `TASK-0147`.
- Security/authority invariants were added and aligned to fail-closed + bounded-input expectations.
- Red-flag section now includes perf-claim drift and explicit mitigation bullets.
- Gate-E alignment is explicit:
  - deterministic marker + assertion proof is required,
  - perf-budget closure is explicitly delegated to `TASK-0056C`.
- Touched-path allowlist was corrected to real repo paths (`source/services/ime`, `docs/dev/ui/input/input.md`, `docs/devx/nx-cli.md`, proof-manifest path) and legacy/non-existent paths were removed.

## Immediate execution focus for TASK-0253

- Implement `hidrawd`, `touchd`, and `inputd` as the single OS/QEMU live-input pipeline using `TASK-0252` crates.
- Wire `windowd`/SystemUI/IME hooks without moving hit-test/focus authority out of `windowd`.
- Add deterministic selftests + marker ladder with reject-path coverage for malformed/stale/unauthorized inputs.
- Preserve non-claims:
  - no perf-budget closure in 0253 (belongs to `TASK-0056C`),
  - no full IME/OSK behavior in 0253 (belongs to `TASK-0146`/`TASK-0147`).
