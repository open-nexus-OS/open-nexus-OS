---
title: TASK-0056B UI v2a extension: visible input v0 (cursor + focus + click) in QEMU
status: In Progress
owner: @ui
created: 2026-03-28
depends-on:
  - TASK-0055C
  - TASK-0056
follow-up-tasks:
  - TASK-0056C
  - TASK-0199
  - TASK-0200
  - TASK-0253
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

After `windowd` becomes visible, the next blocker for meaningful UI/app testing is interaction.
We need the smallest possible visible input slice that proves:

- pointer movement is visible,
- focus changes are visible,
- a click can trigger a real UI response.

This task intentionally stays above the full low-level input device stack. It is the earliest visually testable bridge
from visible shell bring-up to later `inputd`/HID work.

## Goal

Deliver:

1. Visible pointer/cursor v0:
   - render a deterministic software cursor or focus pointer indicator
   - pointer movement updates the visible location in the QEMU window
2. Visible focus model:
   - clicking a surface transfers focus
   - focused surface shows a deterministic visual affordance
3. Minimal click proof:
   - a launcher tile/button/highlight can be clicked and visibly changes state
   - keep the interaction bounded and deterministic

## Non-Goals

- Full HID stack.
- Text entry / IME.
- Drag-and-drop.
- Gesture recognition.
- Rich cursor themes or resize cursors.

## Constraints / invariants (hard requirements)

- No second input model; extend the same routing model as `TASK-0056`.
- Visible cursor/focus must reflect real routing, not a fake overlay disconnected from hit-testing.
- Deterministic pointer sequences in selftests.
- Keep the proof surface tiny: one clickable surface is enough.

## Security / authority invariants

- `windowd` remains the single authority for hit-test, focus transitions, and input delivery.
- Visible cursor/focus state is derived from routed input state, not from client-local overlays.
- Stale/unauthorized surface references remain fail-closed with stable error classes.
- Input event queue and pointer trail state remain bounded to prevent unbounded growth/DoS behavior.
- Markers expose only bounded metadata (surface ids/seq/counters), never raw input payload dumps.

## Red flags / decision points

- **YELLOW (fake overlay risk)**:
  - a cursor drawn from selftest/launcher without routed state would produce fake visual green.
- **YELLOW (marker dishonesty risk)**:
  - `visible ok` markers could appear before real focus/click transition if not post-state gated.
- **YELLOW (scope drift risk)**:
  - 56B can drift into HID stack, latency tuning, or WM-lite semantics.
- **YELLOW (authority drift risk)**:
  - adding a second input lane outside `windowd` would violate 56/50 carry-in.

Red-flag mitigation now:

- require host assertions for routed pointer/focus/click state and visible-state coupling,
- gate visible markers on post-state evidence from `windowd` + proof-surface state,
- keep one `windowd` authority path for input semantics,
- defer HID/IME/perf/WM breadth to explicit follow-up tasks.

## Gate E quality mapping (TRACK alignment)

`TASK-0056B` contributes to Gate E (`Windowing, UI & Graphics`, `production-floor`) by extending
56 from routed-but-nonvisual input semantics to deterministic visible input proof in QEMU:

- visible pointer motion tied to routed pointer state,
- visible focus affordance tied to focus transfer,
- visible click response tied to real routed click delivery.

It must not claim Gate A/B/C/D kernel/core production-grade closure and must not absorb
`TASK-0056C` or `TASK-0199`/`TASK-0200` scope.

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — required

UART markers:

- `windowd: input visible on`
- `windowd: cursor move visible`
- `windowd: focus visible`
- `launcher: click visible ok`
- `SELFTEST: ui visible input ok`

### Host proofs — required

- `cargo test -p ui_v2a_host -- --nocapture` (updated with visible-input assertions)
- `cargo test -p ui_v2a_host reject -- --nocapture` (reject paths for stale/unauthorized/queue bounds)
- `cargo test -p windowd -p launcher -- --nocapture` (regression floor for marker and click coupling)

Visual proof:

- pointer movement is visible in the QEMU window
- clicking the proof surface changes visible state

Quality gates required before `Done`:

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

1. visible cursor/focus affordance in `windowd`-owned render path
2. click proof surface in shell/launcher with post-state marker gating
3. host + reject tests plus visible-bootstrap QEMU marker ladder
4. docs/status sync with explicit non-claims and follow-up scope
