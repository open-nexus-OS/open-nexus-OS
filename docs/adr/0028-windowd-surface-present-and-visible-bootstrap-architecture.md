# ADR-0028: windowd surface/present and visible bootstrap architecture

## Status
Accepted

## Context
The UI stack now has a closed headless baseline (`TASK-0055` / `RFC-0047`) and a closed visible bootstrap follow-up (`TASK-0055B` / `RFC-0048`). We need one clear architecture authority for `windowd` module boundaries, marker honesty, and follow-up scope handoff.

Without a dedicated ADR, module headers and architecture docs risk drifting across:
- `windowd` authority ownership (surface/layer/present sequencing),
- marker semantics (`ok/ready` only after real behavior),
- visible scanout bootstrap boundaries versus future display/input/perf tasks.

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

4. **Marker honesty is mandatory**
   - `windowd`/display success markers are emitted only after verified behavior.
   - Marker-only closure without behavior proof is forbidden.

5. **Capability/security boundary remains explicit**
   - Display/MMIO capability routing remains constrained by the existing device capability model (`TASK-0010`, `RFC-0017`).
   - Invalid mode/format/rights/state requests fail closed and require negative proof coverage.

## Consequences
- **Positive**
  - `windowd` source headers can point to one architecture decision record.
  - UI task slices stay anti-drift: headless closure, visible bootstrap, and follow-up ownership are explicit.
  - Architecture docs can link one canonical ADR instead of duplicating semantics.

- **Negative**
  - Additional doc-sync burden when marker semantics or authority boundaries change.
  - Follow-up tasks must keep explicit scope lines and cannot rely on implicit carry-over.

- **Risks**
  - If module headers are not maintained, ADR/header divergence can still reappear.
  - Overly broad implementation in `TASK-0055B` may violate the narrow bootstrap contract unless gated in review.

## Links
- `docs/rfcs/RFC-0047-ui-v1b-windowd-surface-layer-present-contract.md`
- `docs/rfcs/RFC-0048-ui-v1c-visible-qemu-scanout-bootstrap-contract.md`
- `tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md`
- `tasks/TASK-0055B-ui-v1c-visible-qemu-scanout-bootstrap.md`
- `tasks/TASK-0055C-ui-v1d-windowd-visible-present-systemui-first-frame.md`
- `tasks/TASK-0010-device-mmio-access-model.md`
