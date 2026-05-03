# RFC-0051: UI v2a visible input (cursor + hover + focus + click) contract

- Status: Done
- Owners: @ui
- Created: 2026-04-30
- Last Updated: 2026-05-03
- Links:
  - Tasks: `tasks/TASK-0056B-ui-v2a-visible-input-cursor-focus-click.md` (execution + proof)
  - ADRs: `docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md`
  - Related RFCs:
    - `docs/rfcs/RFC-0049-ui-v1d-windowd-visible-present-systemui-first-frame-contract.md`
    - `docs/rfcs/RFC-0050-ui-v2a-present-scheduler-double-buffer-input-routing-contract.md`

## Status at a Glance

- **Phase 0 (contract freeze + authority boundaries)**: ✅
- **Phase 1 (visible cursor/focus semantics + host proofs)**: ✅
- **Phase 2 (QEMU visible input proof + marker ladder)**: ✅
- **Phase 3 (live QEMU pointer device move/hover/click proof)**: Re-scoped to `TASK-0252`/`TASK-0253`

Definition:

- "Complete" means the contract is defined and the proof gates are green (host tests + OS/QEMU markers). It does not mean immutable forever.

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - deterministic QEMU-visible input-affordance proof (cursor/hover/focus/click) in QEMU.
  - `windowd` authority contract for hit-test, hover, focus, click, and visible affordance state.
  - Marker honesty constraints for visible input success markers.
  - Minimal visual coupling rules: visible state must reflect routed input state, not disconnected marker-only injection.
- **This RFC does NOT own**:
  - host input core (`TASK-0252`),
  - live QEMU HID/touch/keymap/IME input device stack (`TASK-0253`),
  - perf/latency/coalescing tuning (`TASK-0056C`),
  - WM/compositor-v2 breadth (`TASK-0199`/`TASK-0200`),
  - kernel production-grade closure.

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define stop conditions and proof commands.
- `TASK-0056B` is the execution SSOT for this contract seed.

## Context

`TASK-0056` closed routed input semantics (hit-test/focus/keyboard) but did not require persistent visible cursor/focus click affordances in the QEMU window. For practical UI testing and the UI fast lane toward scrolling, animation, windowing, and launcher work, the next smallest contract is deterministic guest-visible routed input state. Live QEMU device input follows immediately in `TASK-0252`/`TASK-0253`.

## Goals

- Define minimal v2a deterministic visible-input contract that keeps `windowd` as the single hit-test/hover/focus/click authority.
- Define deterministic marker and proof requirements so visual input success cannot be faked.

## Non-Goals

- Full HID/touch ingestion pipeline, keymaps, key repeat, and IME/text entry.
- Cursor theme richness, drag/drop, gestures, or WM-lite behavior.

## Constraints / invariants (hard requirements)

- **Determinism**: scripted regression sequences must be deterministic and marker-gated.
- **No fake success**: `visible ok` markers may emit only after routed state and visible state changed.
- **Bounded resources**: pointer trail/input event bookkeeping remains bounded.
- **Security floor**: stale/unauthorized surface references fail closed.
- **Stubs policy**: any visual stub path must be explicitly labeled and cannot emit success markers.

## Proposed design

### Contract / interface (normative)

- A deterministic pointer sequence routed through `windowd` updates a visible cursor in the guest.
- Pointer hover over a committed hit surface updates a deterministic visible hover affordance.
- Focus transfer on click is visibly represented for the focused surface.
- A minimal launcher proof surface changes visible state on routed click.
- Visible markers encode only bounded metadata and are emitted post-state.
- Live host-pointer ingestion is deliberately not claimed here; it is the next architectural slice in `TASK-0252`/`TASK-0253`.

### Phases / milestones (contract-level)

- **Phase 0**: freeze visible-input scope, authority, and marker honesty semantics.
- **Phase 1**: implement visible cursor/hover/focus/click coupling with host + reject proofs.
- **Phase 2**: wire QEMU visible-input marker ladder and verify in visible-bootstrap profile.
- **Phase 3**: re-scoped out of this RFC; `TASK-0252`/`TASK-0253` own live host/QEMU input.

## Security considerations

- **Threat model**: fake visual green via disconnected overlays; deterministic input mistaken for live host input; stale/forged surface ids; unbounded event growth.
- **Mitigations**: keep input authority in `windowd`, fail-closed rejects, bounded queues/state, post-state marker gating.
- **Open risks**: conflating deterministic visible input with live HID/touch/keymap/IME, latency, or WM closure; those remain explicit non-scope.

## Failure model (normative)

- Invalid/stale/unauthorized routing requests must fail closed with stable errors.
- Missing visible-state transition must prevent visible-success markers.
- No silent fallback to fake cursor/focus overlays outside `windowd` authority.
- Live-pointer success markers must not exist in this RFC/task; they belong to `TASK-0253`.

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

This proof uses a deterministic staged selftest pointer/click sequence routed into `windowd`.
Live QEMU pointer/keyboard device proof is required immediately afterward by `TASK-0252`/`TASK-0253`, not by 56B.

### Deterministic markers (if applicable)

- `windowd: input visible on`
- `windowd: cursor move visible`
- `windowd: hover visible`
- `windowd: focus visible`
- `launcher: click visible ok`
- `SELFTEST: ui visible input ok`

### Evidence so far (2026-05-03)

- `cargo test -p ui_v2a_host -- --nocapture` — green, 19 tests.
- `cargo test -p ui_v2a_host reject -- --nocapture` — green, 12 reject-filtered tests.
- `cargo test -p windowd -p launcher -- --nocapture` — green, 15 tests across `windowd` and `launcher`.
- `cargo test -p selftest-client -- --nocapture` — green compile/test check after selftest visible-input wiring.
- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap` — green for the deterministic selftest route.
- Follow-up visual-proof investigation fixed the `selftest-client` QEMU `etc/ramfb`
  config field order to match the required ABI: `addr, fourcc, flags, width, height, stride`.
- Follow-up human-visibility fix scales the tiny `windowd` visible-input frames to the
  fixed 1280x800 `ramfb` mode before scanout and writes a three-stage sequence:
  cursor start position, hover/cursor end position, then final focus/click state.

Closure quality gates confirmed green before final Done claim:

- `scripts/fmt-clippy-deny.sh`
- `just test-all`
- `just ci-network`
- `make clean`, `make build`, `make test`, `make run`

Additional live-input proof deliberately moved to `TASK-0252`/`TASK-0253`:

- Live QEMU pointer device/event source reaches the guest.
- Host mouse move updates `windowd` cursor position live in the QEMU window.
- Host mouse hover produces visible hover affordance.
- Host mouse click produces visible launcher proof-surface state change.
- Live-pointer success markers cannot be satisfied by deterministic selftest injection alone.

## Alternatives considered

- Keep visible input evidence marker-only without host assertions (rejected: high fake-green risk).
- Keep 56B deterministic and pull live pointer immediately into `TASK-0252`/`TASK-0253` (accepted: preserves architecture boundaries while keeping the UI fast lane honest).
- Fold 56B into 56C perf slice (rejected: mixes functional and latency scope, weakens closure clarity).

## Open questions

- Should visible cursor/focus marker payload include minimal sequence ids now or defer to 56C? (owner: @ui, decision in Phase 1)

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 0**: contract freeze + authority boundaries — proof: `task+RFC sync review`
- [x] **Phase 1**: visible cursor/hover/focus/click contract + host/reject proofs — proof: `cargo test -p ui_v2a_host -- --nocapture && cargo test -p ui_v2a_host reject -- --nocapture && cargo test -p windowd -p launcher -- --nocapture`
- [x] **Phase 2**: QEMU visible-input marker ladder wired + verified — proof: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap`
- [x] **Phase 3**: re-scoped to `TASK-0252`/`TASK-0253` — proof will live in those tasks.
- [x] Task(s) linked with stop conditions + proof commands.
- [x] QEMU markers (if any) appear in `scripts/qemu-test.sh` and pass.
- [x] Security-relevant negative tests exist (`test_reject_*`).
