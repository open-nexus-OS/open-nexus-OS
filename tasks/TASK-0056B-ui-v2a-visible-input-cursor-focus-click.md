---
title: TASK-0056B UI v2a extension: visible input v0 (cursor + focus + click) in QEMU
status: Draft
owner: @ui
created: 2026-03-28
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Visible present baseline: tasks/TASK-0055C-ui-v1d-windowd-visible-present-systemui-first-frame.md
  - UI v2a present/input baseline: tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md
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

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — required

UART markers:

- `windowd: input visible on`
- `windowd: cursor move visible`
- `windowd: focus visible`
- `launcher: click visible ok`
- `SELFTEST: ui visible input ok`

Visual proof:

- pointer movement is visible in the QEMU window
- clicking the proof surface changes visible state

## Touched paths (allowlist)

- `source/services/windowd/` + input routing extensions
- SystemUI or launcher proof surface
- `source/apps/selftest-client/`
- `docs/dev/ui/input/input.md`
- `docs/dev/ui/foundations/quality/testing.md`

## Plan (small PRs)

1. visible cursor/focus affordance
2. click proof surface in shell/launcher
3. selftests + docs
