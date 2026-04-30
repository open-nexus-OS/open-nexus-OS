# ADR-0028: windowd surface/present and visible bootstrap architecture

## Status
Accepted

## Context
The UI stack now has a closed headless baseline (`TASK-0055` / `RFC-0047`), a closed visible bootstrap follow-up (`TASK-0055B` / `RFC-0048`), a closed visible SystemUI first-frame slice (`TASK-0055C` / `RFC-0049`), and an in-progress v2a present/input slice (`TASK-0056` / `RFC-0050`). We need one clear architecture authority for `windowd` module boundaries, marker honesty, and follow-up scope handoff.

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

7. **Capability/security boundary remains explicit**
   - Display/MMIO capability routing remains constrained by the existing device capability model (`TASK-0010`, `RFC-0017`).
   - Invalid mode/format/rights/state requests fail closed and require negative proof coverage.

## Consequences
- **Positive**
  - `windowd` source headers can point to one architecture decision record.
  - UI task slices stay anti-drift: headless closure, visible bootstrap, visible SystemUI first frame, and follow-up ownership are explicit.
  - Architecture docs can link one canonical ADR instead of duplicating semantics.
  - v2a present/input keeps a single `windowd` authority path instead of creating launcher/selftest sidecar authority.

- **Negative**
  - Additional doc-sync burden when marker semantics or authority boundaries change.
  - Follow-up tasks must keep explicit scope lines and cannot rely on implicit carry-over.

- **Risks**
  - If module headers are not maintained, ADR/header divergence can still reappear.
  - Overly broad implementation in `TASK-0055B` may violate the narrow bootstrap contract unless gated in review.
  - `TASK-0055C` can regress into fake visibility if marker proofs stop checking the composed `windowd` frame.
  - `TASK-0056` can regress into fake input/present success if marker proofs are accepted without the `ui_v2a_host` behavior assertions.

## Links
- `docs/rfcs/RFC-0047-ui-v1b-windowd-surface-layer-present-contract.md`
- `docs/rfcs/RFC-0048-ui-v1c-visible-qemu-scanout-bootstrap-contract.md`
- `docs/rfcs/RFC-0049-ui-v1d-windowd-visible-present-systemui-first-frame-contract.md`
- `docs/rfcs/RFC-0050-ui-v2a-present-scheduler-double-buffer-input-routing-contract.md`
- `tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md`
- `tasks/TASK-0055B-ui-v1c-visible-qemu-scanout-bootstrap.md`
- `tasks/TASK-0055C-ui-v1d-windowd-visible-present-systemui-first-frame.md`
- `tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md`
- `tasks/TASK-0010-device-mmio-access-model.md`
