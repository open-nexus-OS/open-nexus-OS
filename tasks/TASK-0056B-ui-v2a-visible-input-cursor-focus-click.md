---
title: TASK-0056B UI v2a extension: visible input v0 (cursor + hover + focus + click) in QEMU
status: Done
owner: @ui
created: 2026-03-28
depends-on:
  - TASK-0055C
  - TASK-0056
follow-up-tasks:
  - TASK-0252
  - TASK-0253
  - TASK-0056C
  - TASK-0199
  - TASK-0200
  - TASK-0251
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - RFC seed contract: docs/rfcs/RFC-0051-ui-v2a-visible-input-cursor-focus-click-contract.md
  - Visible present baseline: tasks/TASK-0055C-ui-v1d-windowd-visible-present-systemui-first-frame.md
  - UI v2a present/input baseline: tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md
  - UI v2a contract carry-in: docs/rfcs/RFC-0050-ui-v2a-present-scheduler-double-buffer-input-routing-contract.md
  - Production gates: tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md
  - Input device OS follow-up: tasks/TASK-0253-input-v1_0b-os-hidrawd-touchd-inputd-ime-hooks-selftests.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

After `windowd` becomes visible, the next blocker for meaningful UI/app testing is visible
interaction state. This task proves the smallest deterministic QEMU-visible input surface:

- routed pointer movement changes a visible cursor location,
- routed hover/focus changes are visible,
- a routed click triggers a real UI response.

The real live QEMU device pipeline is intentionally not implemented in 56B. It now follows
immediately in `TASK-0252` (host input core) and `TASK-0253` (OS/QEMU `inputd`/HID pipeline),
so later text, scrolling, animation, and launcher work build on a proper input architecture
instead of a 56B-only inputd-light path.

## Goal

Deliver:

1. Visible pointer/cursor v0:
   - render a deterministic software cursor or focus pointer indicator
   - deterministic routed pointer movement updates the cursor in the QEMU-visible proof
2. Visible focus model:
   - hovering a surface produces a deterministic hover affordance
   - clicking a surface transfers focus
   - focused surface shows a deterministic visual affordance
3. Minimal click proof:
   - a deterministic routed click sequence changes a launcher tile/button/highlight visibly
   - keep the interaction bounded and deterministic
4. Deterministic regression proof:
   - scripted/selftest pointer injection is the 56B proof surface
   - live QEMU pointer/device proof is owned by the immediately following `TASK-0252`/`TASK-0253` lane

## Non-Goals

- Full HID/touch stack or any minimal QEMU pointer device path.
- Keyboard/keymaps/key repeat.
- Text entry / IME.
- Drag-and-drop.
- Gesture recognition.
- Rich cursor themes or resize cursors.

## Constraints / invariants (hard requirements)

- No second input model; extend the same routing model as `TASK-0056`.
- Visible cursor/focus must reflect real routing, not a fake overlay disconnected from hit-testing.
- Deterministic pointer sequences are the 56B closure proof; live QEMU pointer events move to `TASK-0253`.
- Keep the proof surface tiny: one clickable surface is enough.

## Security / authority invariants

- `windowd` remains the single authority for hit-test, focus transitions, and input delivery.
- Visible cursor/focus state is derived from routed input state, not from client-local overlays.
- Future `inputd`/HID services from `TASK-0253` are event sources/normalizers; they must not own hit-test, hover, focus, or click success.
- Stale/unauthorized surface references remain fail-closed with stable error classes.
- Input event queue and pointer trail state remain bounded to prevent unbounded growth/DoS behavior.
- Markers expose only bounded metadata (surface ids/seq/counters), never raw input payload dumps.

## Red flags / decision points

- **YELLOW (fake overlay risk)**:
  - a cursor drawn from selftest/launcher without routed state would produce fake visual green.
- **YELLOW (marker dishonesty risk)**:
  - `visible ok` markers could appear before real focus/click transition if not post-state gated.
- **RED (fake live-input claim risk)**:
  - deterministic selftest `route_pointer_*` calls must not be documented as live host-mouse input.
  - `Done` for 56B may claim only deterministic visible input; live device input is a blocker for `TASK-0253`, not this task.
- **YELLOW (scope drift risk)**:
  - 56B can drift into full HID/touch/keymap/IME stack, latency tuning, or WM-lite semantics.
- **YELLOW (authority drift risk)**:
  - adding a second input lane outside `windowd` would violate 56/50 carry-in.

Red-flag mitigation now:

- require host assertions for routed pointer/focus/click state and visible-state coupling,
- explicitly schedule live QEMU input immediately after 56B via `TASK-0252`/`TASK-0253`,
- gate visible markers on post-state evidence from `windowd` + proof-surface state,
- keep one `windowd` authority path for hit-test/hover/focus/click semantics,
- defer full HID/touch/keymap/IME/perf/WM breadth to explicit follow-up tasks.

## Gate E quality mapping (TRACK alignment)

`TASK-0056B` contributes to Gate E (`Windowing, UI & Graphics`, `production-floor`) by extending
56 from routed-but-nonvisual input semantics to deterministic visible input proof in QEMU:

- visible pointer motion tied to routed pointer state,
- deterministic QEMU-visible cursor movement tied to routed pointer state,
- visible hover affordance tied to hit-test state,
- visible focus affordance tied to focus transfer,
- visible click response tied to real routed click delivery.

It must not claim Gate A/B/C/D kernel/core production-grade closure and must not absorb
`TASK-0252`/`TASK-0253`, `TASK-0056C`, or `TASK-0199`/`TASK-0200`.

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — required

UART markers:

- `windowd: input visible on`
- `windowd: cursor move visible`
- `windowd: hover visible`
- `windowd: focus visible`
- `launcher: click visible ok`
- `SELFTEST: ui visible input ok`

### Host proofs — required

- `cargo test -p ui_v2a_host -- --nocapture` (updated with visible-input assertions)
- `cargo test -p ui_v2a_host reject -- --nocapture` (reject paths for stale/unauthorized/queue bounds)
- `cargo test -p windowd -p launcher -- --nocapture` (regression floor for marker and click coupling)

Visual proof:

- the deterministic proof sequence shows pointer movement in the QEMU window
- hovering the proof surface changes visible hover state
- clicking the proof surface changes visible state
- the QEMU-visible proof must show at least two deterministic pointer locations,
  not just a static final pixel
- live host-mouse interaction is explicitly deferred to the next input tasks, not claimed by 56B

### Evidence so far (2026-05-03)

- Implemented in the existing `windowd` authority path:
  - `windowd`-owned pointer position via routed pointer movement,
  - deterministic cursor pixels and focus affordance in `windowd` composition,
  - launcher visible-click marker gated on `windowd` visible input evidence.
- Host proofs green:
  - `cargo test -p ui_v2a_host -- --nocapture` — 19 tests,
  - `cargo test -p ui_v2a_host reject -- --nocapture` — 12 reject-filtered tests,
  - `cargo test -p windowd -p launcher -- --nocapture` — 15 tests.
- OS/QEMU deterministic proof green:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap` accepted through
    `SELFTEST: ui visible input ok`.
- The QEMU proof writes the 56B `windowd`-composed visible-input frame to the same `ramfb`
  target after the visible SystemUI present baseline. No separate input authority was added.
- Follow-up visual-proof investigation found and fixed a fake-green root cause in
  `selftest-client`: the QEMU `etc/ramfb` config is now written in the required
  `addr, fourcc, flags, width, height, stride` ABI order before markers are accepted.
- Follow-up human-visibility fix scales the tiny `windowd` 64x48 visible-input proof
  to the 1280x800 `ramfb` scanout and writes a three-stage sequence:
  cursor start position, hover/cursor end position, then final focus/click state.
- Scope correction after review: real host-mouse/device input is not integrated into 56B.
  It is moved directly after 56B as `TASK-0252` + `TASK-0253`.
- Closure quality gates are green:
  - `scripts/fmt-clippy-deny.sh`
  - `just test-all`
  - `just ci-network`
  - `make clean`, `make build`, `make test`, `make run` (in order)

## Touched paths (allowlist)

- `source/services/windowd/` + input routing extensions
- SystemUI or launcher proof surface
- `tests/ui_v2a_host/`
- `source/apps/selftest-client/`
- `source/apps/selftest-client/proof-manifest/`
- `scripts/qemu-test.sh`
- `docs/dev/ui/input/input.md`
- `docs/dev/ui/foundations/quality/testing.md`
- `docs/architecture/README.md`
- `tasks/STATUS-BOARD.md`
- `tasks/IMPLEMENTATION-ORDER.md`

## Plan (small PRs)

1. visible cursor/focus/hover affordance in `windowd`-owned render path
2. click proof surface in shell/launcher with post-state marker gating
3. host + reject tests plus deterministic visible-bootstrap QEMU marker ladder
4. docs/status sync with explicit non-claims and immediate `TASK-0252`/`TASK-0253` follow-up scope
