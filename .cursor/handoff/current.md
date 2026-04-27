# Current Handoff: TASK-0055 in review (RFC-0047 Done)

**Date**: 2026-04-27  
**Active review task**: `tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md` — `In Review`
**Completed contract**: `docs/rfcs/RFC-0047-ui-v1b-windowd-surface-layer-present-contract.md` — `Done`
**Completed predecessor**: `tasks/TASK-0054-ui-v1a-cpu-renderer-host-snapshots.md` — `Done`
**Completed predecessor contract**: `docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md` — `Done`
**Gate policy**: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate E: Windowing, UI & Graphics, `production-floor`)  
**Archived predecessor handoff**: `.cursor/handoff/archive/TASK-0054-ui-v1a-cpu-renderer-host-snapshots.md`

## Closeout summary

- Replaced the `source/services/windowd/` checksum scaffold with a bounded headless surface/layer/present state machine.
- Added `source/services/windowd/idl/` Cap'n Proto seed contracts for surface, layer, vsync/present, and input-stub surfaces.
- Added `tests/ui_windowd_host/` as the canonical host proof crate.
- Added `userspace/apps/launcher/` as a minimal first-frame client backed by the same headless smoke path.
- Wired UI markers through `selftest-client`, proof-manifest, `scripts/qemu-test.sh`, and `tools/postflight-ui.sh`.
- Used a tiny headless `desktop` profile (`64x48`, `60Hz`) for QEMU proof to avoid selftest heap exhaustion; this is not a visible display preset.
- Synchronized `TASK-0055` (`In Review`), `RFC-0047` (`Done`), RFC index, UI/testing docs, status board, implementation order, changelog, and `.cursor` workfiles.

## Proof

- `cargo test -p ui_renderer -- --nocapture` — green, 3 tests.
- `cargo test -p ui_host_snap -- --nocapture` — green, 24 tests.
- `cargo test -p ui_host_snap reject -- --nocapture` — green, 14 reject-filtered tests.
- `just diag-host` — green.
- `just test-all` — green.
- `just ci-network` — green repo regression gate only; not TASK-0054 OS-present proof.
- `scripts/fmt-clippy-deny.sh` — green.
- `make clean`, `make build`, `make test`, `make run` — green in order.
- `cargo test -p windowd -p ui_windowd_host -p launcher -p selftest-client -- --nocapture` — green.
- `cargo test -p ui_windowd_host reject -- --nocapture` — green.
- `cargo test -p ui_windowd_host capnp -- --nocapture` — green.
- `cargo test -p selftest-client -- --nocapture` — green.
- `cargo test -p launcher -- --nocapture` — green.
- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os` — green with TASK-0055 UI markers.
- `scripts/fmt-clippy-deny.sh` — green.
- `make build` → `make test` — green; `make test` was run only after a fresh build for the counted gate.
- `make build` → `make run` — green; `make run` was run only after a fresh build for the counted gate.

## Scope guardrails after TASK-0055

- No kernel changes landed in `TASK-0055`.
- No visible scanout claim: visible QEMU output belongs to `TASK-0055B/C`.
- No real input routing/focus/click claim: input remains stub-only until `TASK-0056B`.
- No zero-copy/perf/kernel production-grade claim: route those to `TASK-0054B/C/D`, `TASK-0288`, and `TASK-0290`.
- TASK-0055 markers correspond to checked headless state, not placeholder output.

## Next

- Close `TASK-0055` review, then move queue head to `TASK-0055B` / visible QEMU scanout bootstrap.
- Carry forward TASK-0055 honesty: headless present markers do not prove visible scanout, input routing, display-driver/GPU, perf budgets, or kernel zero-copy production closure.
