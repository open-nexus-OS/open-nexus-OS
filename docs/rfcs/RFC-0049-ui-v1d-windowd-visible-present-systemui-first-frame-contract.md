# RFC-0049: UI v1d windowd visible present + SystemUI first-frame contract seed

- Status: In Progress
- Owners: @ui @runtime
- Created: 2026-04-30
- Last Updated: 2026-04-30
- Links:
  - Tasks: `tasks/TASK-0055C-ui-v1d-windowd-visible-present-systemui-first-frame.md` (execution + proof SSOT)
  - Related RFCs: `docs/rfcs/RFC-0047-ui-v1b-windowd-surface-layer-present-contract.md`, `docs/rfcs/RFC-0048-ui-v1c-visible-qemu-scanout-bootstrap-contract.md`

## Status at a Glance

- **Phase 0 (visible present contract)**: ⬜
- **Phase 1 (SystemUI first-frame contract)**: ⬜
- **Phase 2 (hardening + Gate E sync)**: ⬜

Definition:

- "Complete" means this contract is implemented and the task proof gates are green. It does not imply input/cursor/perf/kernel production-grade closure.

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Implementation planning and proofs live in `TASK-0055C`.

- **This RFC owns**:
  - one deterministic visible `windowd` present contract on top of 55B visible scanout bootstrap,
  - a minimal visible SystemUI first-frame contract (background + shell surface),
  - marker honesty and precondition gating for visible present claims.
- **This RFC does NOT own**:
  - input/cursor/focus/click behavior (`TASK-0056B`),
  - perf/latency/smoothness closure (`TASK-0056C` + kernel lanes),
  - full display service/driver integration (`TASK-0251`),
  - launcher-rich UI flows and start-profile matrix (`TASK-0055D` and later SystemUI slices).

### Relationship to tasks (single execution truth)

- `TASK-0055C` is execution SSOT for stop conditions, touched paths, and proof commands.
- This RFC is contract SSOT for visible present invariants and anti-drift boundaries.

## Context

`TASK-0055` closed deterministic headless present and `TASK-0055B` closed deterministic visible scanout bootstrap. The remaining gap is proving that real `windowd` + SystemUI first-frame output is what reaches the visible surface.

## Goals

- Define a deterministic visible present contract reusing the same `windowd` lifecycle as headless present.
- Define a minimal visible SystemUI first-frame contract with bounded marker evidence.
- Keep scope narrow so this RFC can be completed without absorbing input/perf/display-daemon work.

## Non-Goals

- Rich SystemUI interactions, launcher workflows, or settings surfaces.
- Cursor/pointer rendering or input routing.
- Multi-display/hotplug/virtio-gpu acceleration.
- Kernel production-grade performance claims.

## Constraints / invariants (hard requirements)

- **Determinism**: marker ladder and first-frame content are deterministic for fixed build/profile.
- **No fake success**: visible markers must not emit before real visible present preconditions are met.
- **Bounded resources**: fixed mode/path for this slice; no unbounded buffers/queues/retries.
- **Security floor**: preserve single-authority present path and policy-gated capability boundaries from prior slices.
- **Stubs policy**: placeholders must be explicitly labeled and must not emit success markers.

## Proposed design

### Contract / interface (normative)

- Visible mode remains anchored to the 55B bootstrap baseline (`1280x800`, `argb8888`).
- `windowd` remains present authority and must drive visible present through the same lifecycle used for headless present.
- SystemUI contributes one minimal deterministic first frame (background + shell) on that path.
- Marker contract for this slice:
  - `windowd: backend=visible`
  - `windowd: present visible ok`
  - `systemui: first frame visible`
  - `SELFTEST: ui visible present ok`

### Phases / milestones (contract-level)

- **Phase 0**: visible present contract over existing `windowd` lifecycle with deterministic marker gating.
- **Phase 1**: minimal visible SystemUI first-frame contract with deterministic content.
- **Phase 2**: reject-path hardening + Gate E production-floor sync for this slice.

## Security considerations

- **Threat model**:
  - fake-visible marker emission without real visible present,
  - authority drift through sidecar present/render paths,
  - confused profile semantics (harness marker profile vs. start profile behavior).
- **Mitigations**:
  - single `windowd` present authority and no parallel debug renderer,
  - marker gating on real visible present state transitions,
  - explicit profile boundary wording in task/docs/tests.
- **Open risks**:
  - input/cursor and perf closure remain out of scope until follow-on tasks.

## Failure model (normative)

- Invalid visible mode/capability/present preconditions fail closed with stable error classes.
- Pre-visible success marker emission is rejected.
- No silent fallback from visible contract claims to headless-only success markers.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p windowd -p ui_windowd_host -p systemui -- --nocapture
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap
```

### Deterministic markers

- `windowd: backend=visible`
- `windowd: present visible ok`
- `systemui: first frame visible`
- `SELFTEST: ui visible present ok`

## Alternatives considered

- Keep visible path as bootstrap pattern only:
  - Rejected; does not prove real `windowd`/SystemUI visible output.
- Add a parallel visible renderer path:
  - Rejected; creates authority drift and weakens marker honesty.

## Open questions

- Should this slice lock a specific minimal shell visual pattern in-repo (fixture-style) or defer that strictness to a dedicated SystemUI visual contract? (Owner: @ui)

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [ ] **Phase 0**: visible present contract + marker gating — proof: `cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap`
- [ ] **Phase 1**: minimal visible SystemUI first-frame contract — proof: `cd /home/jenning/open-nexus-OS && cargo test -p systemui -- --nocapture`
- [ ] **Phase 2**: hardening + Gate E sync for this slice — proof: `cd /home/jenning/open-nexus-OS && scripts/fmt-clippy-deny.sh && just test-all && just ci-network`
- [ ] Task linked with stop conditions + proof commands.
- [ ] QEMU markers appear in `scripts/qemu-test.sh` and pass.
- [ ] Security-relevant negative tests exist (`test_reject_*`).
