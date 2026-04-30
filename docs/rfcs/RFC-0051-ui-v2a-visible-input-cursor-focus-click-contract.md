# RFC-0051: UI v2a visible input (cursor + focus + click) contract

- Status: In Progress
- Owners: @ui
- Created: 2026-04-30
- Last Updated: 2026-04-30
- Links:
  - Tasks: `tasks/TASK-0056B-ui-v2a-visible-input-cursor-focus-click.md` (execution + proof)
  - ADRs: `docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md`
  - Related RFCs:
    - `docs/rfcs/RFC-0049-ui-v1d-windowd-visible-present-systemui-first-frame-contract.md`
    - `docs/rfcs/RFC-0050-ui-v2a-present-scheduler-double-buffer-input-routing-contract.md`

## Status at a Glance

- **Phase 0 (contract freeze + authority boundaries)**: 🟨
- **Phase 1 (visible cursor/focus semantics + host proofs)**: ⬜
- **Phase 2 (QEMU visible input proof + marker ladder)**: ⬜

Definition:

- "Complete" means the contract is defined and the proof gates are green (host tests + OS/QEMU markers). It does not mean immutable forever.

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - `windowd`-authority contract for visible input affordance (cursor/focus/click) in QEMU.
  - Marker honesty constraints for visible input success markers.
  - Minimal visual coupling rules: visible state must reflect routed input state.
- **This RFC does NOT own**:
  - full HID/input device stack (`TASK-0253`),
  - perf/latency/coalescing tuning (`TASK-0056C`),
  - WM/compositor-v2 breadth (`TASK-0199`/`TASK-0200`),
  - kernel production-grade closure.

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define stop conditions and proof commands.
- `TASK-0056B` is the execution SSOT for this contract seed.

## Context

`TASK-0056` closed routed input semantics (hit-test/focus/keyboard) but did not require persistent visible cursor/focus click affordances in the QEMU window. For practical UI testing, the next smallest contract is visible, deterministic input feedback tied to real routing state.

## Goals

- Define minimal v2a visible-input contract that keeps `windowd` as single input authority.
- Define deterministic marker and proof requirements so visual input success cannot be faked.

## Non-Goals

- Full HID/touch ingestion pipeline and IME/text entry.
- Cursor theme richness, drag/drop, gestures, or WM-lite behavior.

## Constraints / invariants (hard requirements)

- **Determinism**: pointer/focus/click proof sequences must be deterministic in host and QEMU.
- **No fake success**: `visible ok` markers may emit only after routed state and visible state changed.
- **Bounded resources**: pointer trail/input event bookkeeping remains bounded.
- **Security floor**: stale/unauthorized surface references fail closed.
- **Stubs policy**: any visual stub path must be explicitly labeled and cannot emit success markers.

## Proposed design

### Contract / interface (normative)

- Pointer movement updates a deterministic visible cursor/focus indicator.
- Focus transfer on click is visibly represented for the focused surface.
- A minimal launcher proof surface changes visible state on routed click.
- Visible markers encode only bounded metadata and are emitted post-state.

### Phases / milestones (contract-level)

- **Phase 0**: freeze visible-input scope, authority, and marker honesty semantics.
- **Phase 1**: implement visible cursor/focus/click coupling with host + reject proofs.
- **Phase 2**: wire QEMU visible-input marker ladder and verify in visible-bootstrap profile.

## Security considerations

- **Threat model**: fake visual green via disconnected overlays; stale/forged surface ids; unbounded event growth.
- **Mitigations**: keep input authority in `windowd`, fail-closed rejects, bounded queues/state, post-state marker gating.
- **Open risks**: conflating visible-input floor with HID/latency/WM closure; this remains explicit non-scope.

## Failure model (normative)

- Invalid/stale/unauthorized routing requests must fail closed with stable errors.
- Missing visible-state transition must prevent visible-success markers.
- No silent fallback to fake cursor/focus overlays outside `windowd` authority.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p ui_v2a_host -- --nocapture
cd /home/jenning/open-nexus-OS && cargo test -p ui_v2a_host reject -- --nocapture
cd /home/jenning/open-nexus-OS && cargo test -p windowd -p launcher -- --nocapture
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap
```

### Deterministic markers (if applicable)

- `windowd: input visible on`
- `windowd: cursor move visible`
- `windowd: focus visible`
- `launcher: click visible ok`
- `SELFTEST: ui visible input ok`

## Alternatives considered

- Keep visible input evidence marker-only without host assertions (rejected: high fake-green risk).
- Fold 56B into 56C perf slice (rejected: mixes functional and latency scope, weakens closure clarity).

## Open questions

- Should visible cursor/focus marker payload include minimal sequence ids now or defer to 56C? (owner: @ui, decision in Phase 1)

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [ ] **Phase 0**: contract freeze + authority boundaries — proof: `task+RFC sync review`
- [ ] **Phase 1**: visible cursor/focus/click contract + host/reject proofs — proof: `cargo test -p ui_v2a_host -- --nocapture && cargo test -p ui_v2a_host reject -- --nocapture && cargo test -p windowd -p launcher -- --nocapture`
- [ ] **Phase 2**: QEMU visible-input marker ladder wired + verified — proof: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap`
- [ ] Task(s) linked with stop conditions + proof commands.
- [ ] QEMU markers (if any) appear in `scripts/qemu-test.sh` and pass.
- [ ] Security-relevant negative tests exist (`test_reject_*`).
