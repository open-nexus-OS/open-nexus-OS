# Next Task Preparation (Drift-Free)

## Active execution snapshot (TASK-0252 / RFC-0052)

- **active task**: `tasks/TASK-0252-input-v1_0a-host-hid-touch-keymaps-repeat-accel-deterministic.md` — `In Progress`.
- **active contract seed**: `docs/rfcs/RFC-0052-input-v1_0a-host-hid-touch-keymaps-repeat-accel-contract.md` — `In Progress`.
- **proof posture**: host-first Soll behavior + `test_reject_*` first; no marker-only closure in 0252.
- **scope split locked**:
  - 0252 = host core libraries/tests,
  - 0253 = OS/QEMU `hidrawd`/`touchd`/`inputd` + live marker ladder.

## TASK-0252 prep hardening (2026-05-03)

- [x] `.cursor/handoff/current.md` archived to `.cursor/handoff/archive/TASK-0056B-ui-v2a-visible-input-cursor-focus-click.md`.
- [x] `TASK-0252` moved to `In Progress` and explicitly depends on `TASK-0056B`.
- [x] `TASK-0252` now includes a dedicated security/authority invariants section.
- [x] Red-flag coverage now includes scope drift and explicit mitigations.
- [x] Gate-E (`production-floor`) mapping added via `TRACK-PRODUCTION-GATES-KERNEL-SERVICES`.
- [x] Touched-path drift fixed (`userspace/*` crate layout and `docs/dev/ui/input/input.md` docs path).
- [x] RFC contract seed added and linked: `RFC-0052`.

## Completed execution

- **task**: `tasks/TASK-0054-ui-v1a-cpu-renderer-host-snapshots.md` — `Done`.
- **contract**: `docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md` — `Done`.
- **gate**: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` Gate E (`Windowing, UI & Graphics`, `production-floor`).
- **completed predecessor**: `tasks/TASK-0047-policy-as-code-v1-unified-engine.md` — `Done`.
- **archived predecessor handoff**: `.cursor/handoff/archive/TASK-0054-ui-v1a-cpu-renderer-host-snapshots.md`.

## Current execution snapshot

- **task**: `tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md` — `Done`.
- **contract**: `docs/rfcs/RFC-0050-ui-v2a-present-scheduler-double-buffer-input-routing-contract.md` — `Done`.
- **contract carry-in**: `docs/rfcs/RFC-0049-ui-v1d-windowd-visible-present-systemui-first-frame-contract.md` — `Done`.
- **carry-in baseline**: `TASK-0055` / `RFC-0047` / `TASK-0055B` / `RFC-0048` / `TASK-0055C` / `RFC-0049` are `Done`.

## TASK-0054 closure checks

- [x] Follow-up tasks are now in the header: `TASK-0054B`, `TASK-0054C`, `TASK-0054D`, `TASK-0169`, `TASK-0170`.
- [x] RFC-0046 exists and is linked from TASK-0054 plus `docs/rfcs/README.md`.
- [x] TASK-0054 is marked `Done`; RFC-0046 is marked `Done`.
- [x] `.cursor/context_bundles.md`, `.cursor/pre_flight.md`, and `.cursor/stop_conditions.md` include TASK-0054-specific entries.
- [x] Current-state note matches repo reality: `userspace/ui/renderer/` exists as the narrow host proof floor; `TASK-0169` / `TASK-0170` remain successor scope.
- [x] Security section exists with threat model, invariants, and DON'T DO list.
- [x] Red flags are explicit:
  - `TASK-0169` overlap,
  - host font determinism,
  - PNG/golden determinism,
  - protected root `Cargo.toml` workspace update,
  - production-grade claim boundary.
- [x] Production gate mapping is explicit: TASK-0054 contributes only Gate E `production-floor`, not Gate A kernel/core `production-grade`.
- [x] Reject proof requirements are explicit for oversize inputs, invalid stride/dimensions, damage overflow, golden update gating, and fixture traversal.
- [x] Proof floor is green:
  - `cargo test -p ui_renderer -- --nocapture`
  - `cargo test -p ui_host_snap -- --nocapture`
  - `cargo test -p ui_host_snap reject -- --nocapture`
  - `just diag-host`
  - `just test-all`
  - `just ci-network`
  - `scripts/fmt-clippy-deny.sh`
  - `make clean`, `make build`, `make test`, `make run`

## TASK-0055 closeout snapshot

- **completed task**: `tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md` — `Done`.
- **completed contract**: `docs/rfcs/RFC-0047-ui-v1b-windowd-surface-layer-present-contract.md` — `Done`.
- **proof floor**:
  - `cargo test -p windowd -p ui_windowd_host -p launcher -p selftest-client -- --nocapture`
  - `cargo test -p ui_windowd_host reject -- --nocapture`
  - `cargo test -p ui_windowd_host capnp -- --nocapture`
  - `cargo test -p selftest-client -- --nocapture`
  - `cargo test -p launcher -- --nocapture`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
  - `scripts/fmt-clippy-deny.sh`
  - `make build` → `make test`
  - `make build` → `make run`
- **repo reality**:
  - `source/services/windowd/` contains the bounded headless surface/layer/present state machine,
  - `userspace/apps/launcher/` exists as the minimal first-frame client,
  - UI present markers are wired through `selftest-client`, proof-manifest, `scripts/qemu-test.sh`, and `tools/postflight-ui.sh`.
- **follow-ups remain in header**: `TASK-0055B`, `TASK-0055C`, `TASK-0055D`, `TASK-0056`, `TASK-0056B`, `TASK-0056C`, `TASK-0169`, `TASK-0170`, `TASK-0170B`, `TASK-0250`, `TASK-0251`.
- **Gate E boundary**: TASK-0055 proves headless surface/composition/present only; visible output, input routing, and kernel/MM/IPC/zero-copy production-grade claims remain follow-ups.

## TASK-0055B closure snapshot

- `TASK-0055B` closure-hardening is complete; task and contract are `Done`.
- The implementation targets one deterministic visible QEMU `ramfb` bootstrap path selected by `NEXUS_DISPLAY_BOOTSTRAP=1`.
- The `visible-bootstrap` proof-manifest profile is harness/marker-only; it is not a SystemUI/launcher start profile.
- `windowd` owns mode/present/pattern/marker gating and the composed frame; `selftest-client` writes that frame to the framebuffer VMO and configures `etc/ramfb` through policy-gated `fw_cfg` MMIO.
- Observed marker ladder on closure run: `display: bootstrap on`, `display: mode 1280x800 argb8888`, `windowd: present ok (seq=1 dmg=1)`, `display: first scanout ok`, `SELFTEST: display bootstrap guest ok`.
- Full closure gate sweep is green in sequence: `scripts/fmt-clippy-deny.sh`, `just test-all`, `just ci-network`, `make clean`, `make build`, `make test`, `make run`, plus `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap`.

## Active task prep prompt (TASK-0252)

- Active queue head is `TASK-0252` (input v1.0a host HID/touch/keymaps/repeat/accel core).
- `TASK-0055C`/`RFC-0049` are closed and verified as carry-in.
- `TASK-0056` is `Done`; `RFC-0050` is `Done` as the closed contract authority.
- `TASK-0056B` and `RFC-0051` are `Done` and locked as deterministic visible-input carry-in.
- Preserve scope boundaries after review: 56B is deterministic visible input only; live QEMU pointer/keyboard moves to `TASK-0252`/`TASK-0253`; no perf closure (`TASK-0056C`), no WM/compositor-v2 breadth (`TASK-0199`/`TASK-0200`), no display-service integration closure (`TASK-0251`).
- Implementation checkpoint (deterministic visible-input route):
  - 56B host visible-input proof is green (`cargo test -p ui_v2a_host -- --nocapture`) with 19 tests,
  - 56B reject suite is green (`cargo test -p ui_v2a_host reject -- --nocapture`) with 12 reject-filtered tests,
  - `windowd`/launcher regression proof is green (`cargo test -p windowd -p launcher -- --nocapture`),
  - `selftest-client` compile/test check is green (`cargo test -p selftest-client -- --nocapture`),
  - visible-bootstrap 56B QEMU marker proof is green through `SELFTEST: ui visible input ok` (`RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap`),
  - touched docs/task/RFC/status/handoff notes are synced to partial-closure reality.
- Immediate follow-up after 56B:
  - `TASK-0252` host input core,
  - `TASK-0253` OS/QEMU `inputd`/HID pipeline for live pointer/keyboard,
  - `TASK-0056C` responsiveness after the live path exists.
- Closure gates for `TASK-0056B` are complete: `scripts/fmt-clippy-deny.sh`, `just test-all`, `just ci-network`, and `make clean` -> `make build` -> `make test` -> `make run`.

## Fast-lane uplift checkpoint

- `tasks/IMPLEMENTATION-ORDER.md` now defines an Orbital-Level UX gate before `TASK-0119`/`TASK-0120`; this is a UX bar only, not an adoption of Orbital architecture.
- Carry-in architecture remains Open Nexus/OHOS/Zircon style:
  - `inputd` normalizes QEMU pointer/keyboard events after `TASK-0253`,
  - `windowd` owns hit-test, hover, focus, click, scroll/window interaction authority,
  - SystemUI owns shell/launcher/session surfaces,
  - app/session services own lifecycle/session state.
- New task: `TASK-0065B-session-login-greeter-v0.md` for greeter/dev-session and SystemUI shell handoff.
- `TASK-0146`/`TASK-0147` are pulled directly after `TASK-0059` for IME keymaps, OSK, focus routing, and QEMU proof before the SystemUI DSL desktop claim.
- Downstream UI fast-lane tasks now require live pointer/scroll/launcher proofs and SVG-source UI assets where relevant, so later desktop/launcher tasks cannot fall back to marker-only or PNG-first claims.

## Carry-forward guardrails

- No kernel, compositor, GPU, input-routing, or OS present marker work in TASK-0054.
- No host font discovery or locale-dependent fallback.
- No golden rewriting unless explicitly gated by `UPDATE_GOLDENS=1`.
- No success marker for placeholder behavior.
- No weakening of RFC-0046 proof requirements to fit an easy implementation.
- No fake visible marker closure for TASK-0055B (visual/manual checks cannot replace deterministic marker+harness proof).
- No parallel present/input authority outside `windowd` for TASK-0056.
- No marker-only closure for scheduler/focus semantics; host assertions are mandatory.
