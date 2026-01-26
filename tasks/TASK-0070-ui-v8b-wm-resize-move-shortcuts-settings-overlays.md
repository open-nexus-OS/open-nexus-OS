---
title: TASK-0070 UI v8b: WM edge resize/move + global shortcuts + quick settings/settings overlays (stubs) + tests/markers
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - UI v6a WM baseline: tasks/TASK-0064-ui-v6a-window-management-scene-transitions.md
  - UI v7a snap baseline: tasks/TASK-0066-ui-v7a-wm-split-snap.md
  - UI v7c screencap/share baseline (shortcut): tasks/TASK-0068-ui-v7c-screenshot-screencap-share-sheet.md
  - UI v5b theme tokens baseline (settings toggle): tasks/TASK-0063-ui-v5b-virtualized-list-theme-tokens.md
  - UI v2a input routing baseline: tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md
  - Policy as Code (wm resize constraints): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Config broker (wm/shortcuts flags): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We already have WM basics (v6a) and snap zones (v7a). UI v8b adds:

- pointer-driven window move and edge/corner resize,
- global shortcuts that trigger WM snap, screencap sheet, and settings overlay,
- quick settings/settings overlays (stubs) for operator-friendly UX.

Notifications v2 lives in v8a (`TASK-0069`).
Notifications v2 “deluxe” follow-ups are `TASK-0123` (persistence/history/unread), `TASK-0124` (dndd), and `TASK-0125` (heads-up/redaction/badging/settings + OS proofs).

## Goal

Deliver:

1. WM move + edge resize:
   - hit-zones for edges/corners (desktop)
   - title bar drag to move
   - constraints to display bounds
   - enforce min-size from policy
2. WM IPC additions:
   - `beginMove(win,x,y)`, `beginResize(win,edge,x,y)`, `endMoveResize(win)`
3. Global shortcuts (SystemUI desktop):
   - `Super+Left/Right/Up/Down` → snap zones (reuse v7a)
   - `Super+Shift+S` → open screenshot sheet (reuse v7c; gated)
   - `Super+,` → open settings overlay
4. Settings overlays (stubs):
   - quick settings: Wi‑Fi/Bluetooth/DND stubs, brightness/volume sliders (no backend), theme switch (if v5b exists)
   - system settings stub routes (Network/Display/Accounts placeholders)
5. Host tests + OS markers.

## Non-Goals

- Kernel changes.
- Real backend implementations for Wi‑Fi/Bluetooth/brightness/volume (stubs only).
- Full window resizing decorations and cursors (basic only).

## Constraints / invariants (hard requirements)

- Deterministic move/resize math given an input sequence.
- Respect policy min sizes; deny/clip as documented.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v8b_host/`:

- resize/move:
  - simulate edge drags → resulting bounds respect min size and remain on-screen
  - move keeps window within display bounds
- shortcuts:
  - simulate key chords → snap called, settings overlay opened, screenshot sheet invoked (when present)

### Proof (OS/QEMU) — gated

UART markers (order tolerant):

- `windowd: wm resize on`
- `windowd: wm move on`
- `windowd: wm resized (win=.. w=.. h=..)`
- `windowd: wm moved (win=.. x=.. y=..)`
- `systemui: shortcuts on`
- `systemui: quick settings open`
- `systemui: settings open`
- `SELFTEST: ui v8 resize ok`
- `SELFTEST: ui v8 move ok`
- `SELFTEST: ui v8 settings ok`

## Touched paths (allowlist)

- `source/services/windowd/` + `idl/wm.capnp` (move/resize)
- SystemUI plugins (shortcuts + overlays)
- `tests/ui_v8b_host/`
- `source/apps/selftest-client/`
- `tools/postflight-ui-v8b.sh` (delegates)
- `docs/dev/ui/wm-resize-move.md` + `docs/dev/ui/shortcuts.md`

## Plan (small PRs)

1. WM move/resize zones + IDL + markers
2. shortcuts handling + overlay stubs + markers
3. host tests + OS selftest markers + docs + postflight
