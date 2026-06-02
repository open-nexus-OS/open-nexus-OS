# ADR-0028: windowd surface/present and visible bootstrap architecture

## Status
Accepted (amended 2026-06-02: GPU-only architecture per RFC-0059 Phase 6)

## Context
The UI stack now has a closed headless baseline (`TASK-0055` / `RFC-0047`), a closed visible bootstrap follow-up (`TASK-0055B` / `RFC-0048`), a closed visible SystemUI first-frame slice (`TASK-0055C` / `RFC-0049`), a closed v2a present/input slice (`TASK-0056` / `RFC-0050`), and a closed deterministic visible-input slice (`TASK-0056B` / `RFC-0051`). We need one clear architecture authority for `windowd` module boundaries, marker honesty, and follow-up scope handoff.

Without a dedicated ADR, module headers and architecture docs risk drifting across:
- `windowd` authority ownership (surface/layer/present sequencing),
- marker semantics (`ok/ready` only after real behavior),
- visible scanout/SystemUI first-frame boundaries versus v2a present/input and future display/perf tasks.

## Decision
Adopt a dedicated `windowd` architecture contract with these rules:

1. **Single authority**
   - `windowd` is the authority for surface IDs, layer membership/order, scene commits, and present sequencing.
   - No parallel compositor or second display authority is introduced in bootstrap slices.

2. **Headless baseline is stable**
   - Headless present behavior and reject rules are owned by `RFC-0047` and proven by `TASK-0055`.
   - Headless completion must not be interpreted as visible output/input closure.

3. **Visible bootstrap is incremental**
   - Visible scanout bootstrap is a narrow fixed-mode extension under `RFC-0048` / `TASK-0055B`.
   - `visible-bootstrap` is a harness/marker profile for the QEMU proof, not a future SystemUI/launcher start profile.
   - Bootstrap remains fixed-mode and deterministic; richer display/input/perf behavior is routed to follow-up tasks.

4. **Visible SystemUI first-frame reuses `windowd` composition**
   - `TASK-0055C` must not write a raw SystemUI source buffer directly as the proof of visible present.
   - Visible success evidence is the `windowd`-composed frame produced after scene commit/present sequencing.
   - The initial SystemUI profile/shell seed is TOML-backed and deliberately minimal; richer presets and DSL shells remain follow-up scope.

5. **Marker honesty is mandatory**
   - `windowd`/display success markers are emitted only after verified behavior.
   - Marker-only closure without behavior proof is forbidden.

6. **v2a present/input remains inside `windowd`**
   - `TASK-0056` extends the same authority path with frame-indexed back buffers, deterministic scheduler ticks, minimal post-present fences, and committed-scene hit-test/focus routing.
   - Launcher and selftest are proof consumers only; they do not own present scheduling, hit-test, focus, or input delivery authority.
   - The v2a QEMU proof uses the existing `visible-bootstrap` harness profile with stricter marker expectations; it is still not a desktop/start-profile or screenshot/GTK refresh proof.

7. **Visible input remains deterministic until the input pipeline lands**
   - `TASK-0056B` extends the same `windowd` authority path with deterministic cursor, hover, focus, and click-visible affordances.
   - `visible-bootstrap` may write multiple bounded `windowd`-composed proof frames to the GPU scanout VMO (cursor start, hover/cursor move, final focus/click), but must not claim live host input.
   - Live QEMU pointer/keyboard input is owned by `TASK-0252`/`TASK-0253`; those services feed `windowd` but do not own hit-test, hover, focus, or click success.

8. **Capability/security boundary remains explicit**
   - Display/MMIO capability routing remains constrained by the existing device capability model (`TASK-0010`, `RFC-0017`).
   - Invalid mode/format/rights/state requests fail closed and require negative proof coverage.

9. **Live display output is service-gated**
   - The live chain is `hidrawd -> inputd -> windowd -> gpud (virtio-gpu)`.
   - `selftest-client` remains observer-only and may only emit display/input proof markers after polling service-owned `VisibleState`.
   - A missing stable marker in `visible-bootstrap` is treated as a missing host/service proof for the first broken hop.
   - Current fast-closure matrix: `docs/testing/display-output-hardening-matrix.md`.

10. **TASK-0057 grows `windowd` into Minimal DisplayServer v0**
   - `windowd` is now a standalone os-lite service, not only an in-process library.
   - `windowd` creates its own framebuffer VMO and hands it off to `gpud` for scanout.
   - `windowd` owns JPEG-sourced wallpaper, the Mocu SVG cursor,
     Inter-rendered text/icon proof targets, focus/hit-test state, and
     composed-frame writes.
   - `inputd` sends bounded visible-input updates to `windowd`; it does not own
     cursor pixels or a second display scene.
   - Hover/click/keyboard/wheel target highlights are transient service state,
     not success latches: wheel pulses preserve up/down direction and expire via
     a bounded inputd tick.
   - `SELFTEST: ui v2b assets ok` is valid only after visible input plus
     service-owned cursor/wallpaper/text/icon/overlay evidence.

## Current State

- `TASK-0056` / `RFC-0050` and `TASK-0056B` / `RFC-0051` are `Done`.
- `TASK-0056B` currently claims only deterministic visible input in QEMU: routed cursor movement, hover affordance, focus affordance, and click-visible proof surface state.
- `TASK-0252` has now landed the host-first input core (`hid`, `touch`, `keymaps`, `key-repeat`,
  `pointer-accel`) plus `tests/input_v1_0_host/` as the canonical host proof package.
- `TASK-0253` / `RFC-0053` / `RFC-0054` are now review-closed for the service-owned
  live chain: `virtio-input -> hidrawd -> inputd -> windowd -> gpud`,
  with `selftest-client` kept observer-only.
- `TASK-0057` has lifted the visible asset slice into a Minimal DisplayServer v0:
  `inputd -> windowd -> gpud` is the authoritative live chain, and
  `visible-bootstrap` now gates on v2b asset evidence rather than the older wheel
  marker alone.
- The OS v2b text and cursor assets come from checked-in resource submodules:
  `resources/fonts/inter/docs/font-files/InterVariable.ttf` is rasterized at
  build time for the proof overlay, and `resources/cursors/mocu/src/svg/default.svg`
  is normalized into the bounded SVG subset before rasterization.
- The next follow-up remains broader production display closure; this ADR still
  does not claim GPU, Wayland, multi-window WM, full text input/IME, or kernel/core
  production-grade display closure.

## Consequences
- **Positive**
  - `windowd` source headers can point to one architecture decision record.
  - UI task slices stay anti-drift: headless closure, visible bootstrap, visible SystemUI first frame, and follow-up ownership are explicit.
  - Architecture docs can link one canonical ADR instead of duplicating semantics.
  - v2a present/input keeps a single `windowd` authority path instead of creating launcher/selftest sidecar authority.
  - 56B visible input keeps marker honesty while preserving the architecture boundary for the real input pipeline.
  - TASK-0057 removes the live white-cursor-square truth by routing visible output
    through `windowd`'s SVG cursor and JPEG-sourced root scene.

- **Negative**
  - Additional doc-sync burden when marker semantics or authority boundaries change.
  - Follow-up tasks must keep explicit scope lines and cannot rely on implicit carry-over.

- **Risks**
  - If module headers are not maintained, ADR/header divergence can still reappear.
  - Overly broad implementation in `TASK-0055B` may violate the narrow bootstrap contract unless gated in review.
  - `TASK-0055C` can regress into fake visibility if marker proofs stop checking the composed `windowd` frame.
  - `TASK-0056` can regress into fake input/present success if marker proofs are accepted without the `ui_v2a_host` behavior assertions.
  - `TASK-0056B` can regress into fake visible input if cursor/hover/focus/click markers are emitted without `windowd`-composed frame evidence or if deterministic input is described as live host input.
  - `TASK-0057` can regress into fake asset success if `selftest-client` emits the
    v2b summary without observing `windowd` and `gpud` service-owned asset state.

## Links
- `docs/rfcs/RFC-0047-ui-v1b-windowd-surface-layer-present-contract.md`
- `docs/rfcs/RFC-0048-ui-v1c-visible-qemu-scanout-bootstrap-contract.md`
- `docs/rfcs/RFC-0049-ui-v1d-windowd-visible-present-systemui-first-frame-contract.md`
- `docs/rfcs/RFC-0050-ui-v2a-present-scheduler-double-buffer-input-routing-contract.md`
- `docs/rfcs/RFC-0051-ui-v2a-visible-input-cursor-focus-click-contract.md`
- `tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md`
- `tasks/TASK-0055B-ui-v1c-visible-qemu-scanout-bootstrap.md`
- `tasks/TASK-0055C-ui-v1d-windowd-visible-present-systemui-first-frame.md`
- `tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md`
- `tasks/TASK-0056B-ui-v2a-visible-input-cursor-focus-click.md`
- `tasks/TASK-0057-ui-v2b-text-shaping-svg-pipeline.md`
- `tasks/TASK-0010-device-mmio-access-model.md`
