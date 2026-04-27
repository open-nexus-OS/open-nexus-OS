# Current Handoff: TASK-0055 prep

**Date**: 2026-04-27  
**Active task**: `tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md` — `In Progress`
**Active contract**: `docs/rfcs/RFC-0047-ui-v1b-windowd-surface-layer-present-contract.md` — `In Progress`
**Completed predecessor**: `tasks/TASK-0054-ui-v1a-cpu-renderer-host-snapshots.md` — `Done`
**Completed predecessor contract**: `docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md` — `Done`
**Gate policy**: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate E: Windowing, UI & Graphics, `production-floor`)  
**Archived predecessor handoff**: `.cursor/handoff/archive/TASK-0054-ui-v1a-cpu-renderer-host-snapshots.md`

## Prep summary

- Archived the completed TASK-0054 handoff.
- `TASK-0055` is now `In Progress`; next session should start in Plan Mode before implementation.
- Created `RFC-0047` as the contract seed for `windowd` surface/layer/present semantics and linked it from `TASK-0055` / RFC index.
- Header dependencies now include `TASK-0054`, `TASK-0031`, `TASK-0013`, `TASK-0046`, and `TASK-0047`.
- Header follow-ups now include `TASK-0055B`, `TASK-0055C`, `TASK-0055D`, `TASK-0056`, `TASK-0056B`, `TASK-0056C`, `TASK-0169`, `TASK-0170`, `TASK-0170B`, `TASK-0250`, and `TASK-0251`.
- Current repo state is documented: `source/services/windowd/` exists only as a placeholder checksum/helper scaffold; `userspace/apps/launcher/` and UI-present markers do not exist yet.
- Security/authority section now requires fail-closed VMO/surface/layer IPC handling, caller identity from service metadata, bounded logs, and `test_reject_*` coverage.
- Red flags are clarified: VMO baseline is partly de-risked by predecessor work but still must be proven at the UI boundary; present fences are minimal acknowledgements; visible output remains `TASK-0055B/C`; dev presets remain `TASK-0055D`.
- Gate E mapping is explicit: `TASK-0055` contributes headless surface/composition/present behavior only; visible display, input routing, and kernel/MM/IPC production-grade performance stay in follow-ups.

## Carry-in proof

- `cargo test -p ui_renderer -- --nocapture` — green, 3 tests.
- `cargo test -p ui_host_snap -- --nocapture` — green, 24 tests.
- `cargo test -p ui_host_snap reject -- --nocapture` — green, 14 reject-filtered tests.
- `just diag-host` — green.
- `just test-all` — green.
- `just ci-network` — green repo regression gate only; not TASK-0054 OS-present proof.
- `scripts/fmt-clippy-deny.sh` — green.
- `make clean`, `make build`, `make test`, `make run` — green in order.

## Scope guardrails for TASK-0055

- No kernel changes in the base `TASK-0055` slice.
- No visible scanout claim in `TASK-0055`; visible QEMU output belongs to `TASK-0055B/C`.
- No real input routing/focus/click claim; input is stub-only until `TASK-0056B`.
- No zero-copy/perf/kernel production-grade claim; route those to `TASK-0054B/C/D`, `TASK-0288`, and `TASK-0290`.
- No fake markers: `windowd: ready`, `windowd: present ok`, launcher, and `SELFTEST: ui ... ok` markers must correspond to real checked behavior.

## Next

- Request Plan Mode for `TASK-0055`.
- Read `TASK-0055`, this handoff, `.cursor/current_state.md`, `.cursor/next_task_prep.md`, `TASK-0054`, `RFC-0046`, and Gate E in `TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`.
- Treat `RFC-0047` as the contract seed and `TASK-0055` as execution/proof SSOT.
