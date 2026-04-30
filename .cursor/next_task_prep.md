# Next Task Preparation (Drift-Free)

## Completed execution

- **task**: `tasks/TASK-0054-ui-v1a-cpu-renderer-host-snapshots.md` — `Done`.
- **contract**: `docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md` — `Done`.
- **gate**: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` Gate E (`Windowing, UI & Graphics`, `production-floor`).
- **completed predecessor**: `tasks/TASK-0047-policy-as-code-v1-unified-engine.md` — `Done`.
- **archived predecessor handoff**: `.cursor/handoff/archive/TASK-0054-ui-v1a-cpu-renderer-host-snapshots.md`.

## Active execution

- **task**: `tasks/TASK-0055C-ui-v1d-windowd-visible-present-systemui-first-frame.md` — `In Progress`.
- **contract**: `docs/rfcs/RFC-0049-ui-v1d-windowd-visible-present-systemui-first-frame-contract.md` — `In Progress` (execution task SSOT link established).
- **contract carry-in**: `docs/rfcs/RFC-0048-ui-v1c-visible-qemu-scanout-bootstrap-contract.md` — `Done`.
- **carry-in baseline**: `TASK-0055` / `RFC-0047` are `Done` and remain the headless-only proof floor.

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

## Active task prep prompt (TASK-0055C)

- Current execution SSOT is `TASK-0055C` visible SystemUI first frame in QEMU.
- Contract seed for this slice is `RFC-0049` (`In Progress`), with `RFC-0048` retained as carry-in visible-bootstrap baseline.
- Implementation is partially proven:
  - minimal TOML-backed SystemUI `desktop` profile/shell seed exists,
  - visible `windowd` present composes the deterministic SystemUI first frame into the visible 1280x800 frame on host and exposes composed rows for OS/QEMU,
  - `selftest-client` writes `windowd`-composed rows to QEMU `ramfb`, not a raw SystemUI source buffer or selftest-owned sidecar composition,
  - host required proof, reject proof, OS-target selftest build, QEMU visible marker proof, and `scripts/fmt-clippy-deny.sh` are green.
- Closure remains pending by operator hold:
  - `just test-all`,
  - `just ci-network`,
  - `make clean`, `make build`, `make test`, `make run`.
- Carry forward TASK-0055C honesty: this proves visible SystemUI first-frame only, not input, cursor, GPU, dirty-rect display service behavior, perf budgets, full display integration, dev-preset/start-profile matrix, or kernel/core production-grade display closure.
- Keep startup profile/dev-preset semantics separate from the `visible-bootstrap` harness marker profile.

## Carry-forward guardrails

- No kernel, compositor, GPU, input-routing, or OS present marker work in TASK-0054.
- No host font discovery or locale-dependent fallback.
- No golden rewriting unless explicitly gated by `UPDATE_GOLDENS=1`.
- No success marker for placeholder behavior.
- No weakening of RFC-0046 proof requirements to fit an easy implementation.
- No fake visible marker closure for TASK-0055B (visual/manual checks cannot replace deterministic marker+harness proof).
