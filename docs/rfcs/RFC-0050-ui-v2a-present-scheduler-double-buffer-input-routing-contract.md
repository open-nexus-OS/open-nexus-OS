# RFC-0050: UI v2a present scheduler + double-buffer + input routing contract

- Status: Done
- Owners: @ui
- Created: 2026-04-30
- Last Updated: 2026-04-30
- Links:
  - Tasks: `tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md` (execution + proof)
  - ADRs: `docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md`
  - Related RFCs:
    - `docs/rfcs/RFC-0047-ui-v1b-windowd-surface-layer-present-contract.md`
    - `docs/rfcs/RFC-0048-ui-v1c-visible-qemu-scanout-bootstrap-contract.md`
    - `docs/rfcs/RFC-0049-ui-v1d-windowd-visible-present-systemui-first-frame-contract.md`

## Status at a Glance

- **Phase 0 (contract freeze + authority boundaries)**: ✅
- **Phase 1 (scheduler + fence semantics + proofs)**: ✅
- **Phase 2 (input routing + focus semantics + proofs)**: ✅

Definition:

- "Complete" means the contract is defined and the proof gates are green (host tests + OS/QEMU markers). It does not mean immutable forever.
- RFC contract phases are implemented and proven; this contract is now closed as `Done`.

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - `windowd` contract for double-buffered surface present sequencing,
  - minimal v2a fence semantics (post-present signal, versioned and bounded),
  - deterministic hit-test/focus/input-routing contract for v2a,
  - marker honesty and bounded-state invariants for scheduler and input paths.
- **This RFC does NOT own**:
  - visible cursor/focus polish (`TASK-0056B`),
  - present/input perf tuning and latency optimization closure (`TASK-0056C`),
  - compositor v2 breadth like occlusion/screencap/WM-lite/alt-tab (`TASK-0199`/`TASK-0200`),
  - kernel production-grade scheduler/MM/IPC/zero-copy closure.

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define stop conditions and proof commands.
- `TASK-0056` is the execution SSOT for this contract seed.

## Context

`TASK-0055C` closed visible present + first SystemUI frame. The next Gate-E baseline gap is deterministic real-time behavior:

- back-buffered surface submission,
- vsync-aligned present scheduling with bounded coalescing/fence semantics,
- deterministic input hit-testing/focus routing through `windowd`.

Without this contract, marker ladders can appear green while routing/authority/fence semantics drift.

## Goals

- Define minimal stable v2a contract for double-buffered present in `windowd`.
- Define deterministic and bounded scheduler/fence semantics for host + QEMU proofing.
- Define deterministic input routing/focus semantics that remain single-authority in `windowd`.

## Non-Goals

- Full WM behavior (snap/alt-tab/compositor-v2 scene management).
- Cursor theme/pointer UX polish and latency tuning follow-ups.
- Hardware vsync, GPU backend lock-in, or kernel scheduler redesign.

## Constraints / invariants (hard requirements)

- **Determinism**: host and QEMU proofs must assert real routing/present outcomes, not marker grep only.
- **No fake success**: no `ok/ready` marker before actual state transition (present committed, focus updated, click delivered).
- **Bounded resources**: queue depth, coalesced damage count, and input event backlog are capped with stable reject behavior.
- **Security floor**: stale/unauthorized surface references are rejected fail-closed; routing authority is not payload-derived.
- **Stubs policy**: any temporary stub is explicit, non-authoritative, and cannot emit success markers.

## Proposed design

### Contract / interface (normative)

- Surface present path exposes a back-buffer acquisition and frame-indexed present request.
- Present scheduler:
  - aligns to timer-driven vsync spine,
  - coalesces rapid submits deterministically,
  - emits a minimal versioned fence signal only after the present tick processes the frame.
- Input routing:
  - hit-test walks top-to-bottom against committed scene/layer ordering,
  - focus follows click and keyboard delivery targets focused surface only,
  - stale/unknown surface IDs are rejected with stable error classes.
- Marker contract (summary-only, bounded metadata):
  - `windowd: present scheduler on`
  - `windowd: input on`
  - `windowd: focus -> <surface_id>`
  - `launcher: click ok`
  - `SELFTEST: ui v2 present ok`
  - `SELFTEST: ui v2 input ok`

### Phases / milestones (contract-level)

- **Phase 0**: freeze v2a authority boundaries + marker honesty + boundedness contract.
- **Phase 1**: land deterministic present scheduler + minimal fence contract with host and QEMU proofs.
- **Phase 2**: land deterministic hit-test/focus/keyboard routing contract with host and QEMU proofs.

## Security considerations

- **Threat model**:
  - confused-deputy routing (caller influences focus for non-owned surface),
  - fake-green marker paths that claim present/input success without real state transitions,
  - unbounded queues or event floods causing scheduler/input DoS.
- **Mitigations**:
  - single `windowd` authority for scene/present/focus transitions,
  - stable reject paths for stale/unauthorized/invalid surface references,
  - bounded queue/event/fence state and deterministic reject behavior under pressure,
  - marker strings limited to stable labels and bounded metadata.
- **Open risks**:
  - minimal fence semantics are not low-latency-grade and must stay labeled as v2a baseline only,
  - CPU-only proof path can hide backend coupling if interfaces are not kept backend-agnostic.

## Failure model (normative)

- Invalid frame index, stale present sequence, or unauthorized surface reference: reject with stable error.
- Scheduler overflow or damage/event cap exceeded: reject/defer deterministically; no silent drop as success.
- No-damage/no-state-change path: skip compose/present deterministically; do not emit present-success markers.
- Fallback behavior is explicit and proven; no implicit success fallback.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p ui_v2a_host -- --nocapture
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap
```

### Deterministic markers (if applicable)

- `windowd: present scheduler on`
- `windowd: input on`
- `windowd: focus -> <surface_id>`
- `launcher: click ok`
- `SELFTEST: ui v2 present ok`
- `SELFTEST: ui v2 input ok`

### Evidence so far (2026-04-30)

- Closure rerun `cargo test -p windowd -p launcher -p ui_v2a_host -- --nocapture` — green, 22 tests across the three target packages.
- Closure rerun `cargo test -p ui_v2a_host reject -- --nocapture` — green, 5 reject-filtered tests.
- `cargo test -p windowd -p ui_windowd_host -p launcher -p selftest-client -- --nocapture` — green.
- OS-target visible selftest build check with `NEXUS_DISPLAY_BOOTSTRAP=1` — green.
- Closure rerun `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap` — green; `verify-uart` accepted the visible-bootstrap profile through `SELFTEST: ui v2 input ok`.
- Closure sync updated touched headers, ADR/architecture/testing docs, task/RFC notes, and input-ok marker gating.

Deferred by explicit user instruction before final closure:

- `scripts/fmt-clippy-deny.sh`
- `just test-all`
- `just ci-network`
- `make clean`, `make build`, `make test`, `make run`

## Alternatives considered

- Extend 55C marker-only ladder without host-side routing assertions (rejected: high fake-green risk).
- Split scheduler and input into separate baseline tasks before contract freeze (rejected for now: fragments authority semantics and slows Gate-E baseline closure).

## Decisions during implementation

- Fence timeout classification is deferred to v2c/perf hardening; v2a exposes minimal post-present fence status only.
- v2a uses the existing `visible-bootstrap` harness profile with stricter marker tail wiring instead of adding a new `ui-v2a` profile in this slice.
- The QEMU proof remains UART/proof-manifest evidence, not an independent screenshot or GTK refresh proof.

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 0**: authority boundaries + boundedness + marker honesty frozen in `TASK-0056` and this RFC — proof: `docs+task sync review`
- [x] **Phase 1**: deterministic present scheduler + minimal fence semantics proven — proof: `cargo test -p ui_v2a_host -- --nocapture`
- [x] **Phase 2**: deterministic input hit-test/focus/keyboard routing proven — proof: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap`
- [x] Task(s) linked with stop conditions + proof commands.
- [x] QEMU markers (if any) appear in `scripts/qemu-test.sh` and pass.
- [x] Security-relevant negative tests exist (`test_reject_*`).
