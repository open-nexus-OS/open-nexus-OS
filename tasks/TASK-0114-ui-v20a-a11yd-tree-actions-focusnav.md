---
title: TASK-0114 UI v20a: Accessibility tree v1 hardening (a11yd) + actions + focus/keyboard navigation + adapters
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - A11y v4 baseline: tasks/TASK-0061-ui-v4b-gestures-a11y-semantics.md
  - Design kit (roles/labels): tasks/TASK-0073-ui-v10a-design-system-primitives-goldens.md
  - Window/input baseline: tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md
  - Config broker (a11y prefs): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We already introduced an accessibility semantics direction (`a11yd`) in UI v4b. UI v20 turns this into an
actionable system-wide suite:

- unified tree registration and snapshots,
- standard actions (activate/toggle/focus/scroll/setValue),
- stable focus events,
- keyboard navigation and focus rings across SystemUI and apps.

Screen reader, magnifier/filters, and captions are separate tasks.

## Goal

Deliver:

1. `a11yd` hardening:
   - stable IDL (`a11y.capnp`) with:
     - `registerApp`, `tree`, `focus`, `setFocus`, `onEvent`
     - app-side `snapshot`, `doAction`, `hitTest`
   - deterministic focus event stream
   - markers:
     - `a11yd: ready`
     - `a11y: app registered (id=...)`
     - `a11y: focus (app=.. id=..)`
2. Adapters:
   - SystemUI and `ui/kit` primitives export roles/names and respond to `doAction`
   - ensure all actionable controls have default a11y names/roles
3. Focus & keyboard navigation:
   - Tab/Shift+Tab order (layout order + optional tabIndex hint)
   - Arrow navigation for menus/lists
   - focus ring visuals and a11y focus events
   - global focus shortcuts (window cycle) are optional
   - markers:
     - `focus: next`
     - `focus: prev`
     - `focus: window-cycle`
4. Host tests for tree stability and focus traversal deterministically.

## Non-Goals

- Kernel changes.
- Speech/TTS (v20b).
- Magnifier/filters (v20c).

## Constraints / invariants

- Deterministic traversal order and stable node snapshots.
- Bounded tree sizes per app (caps; documented).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v20a_host/`:

- mock app registers; snapshot JSON stable
- focus traversal Tab/Shift+Tab yields expected order
- doAction(activate/toggle/setValue) routes to app and returns ok deterministically

### Proof (OS/QEMU) — gated

UART markers:

- `a11yd: ready`
- `SELFTEST: ui v20 focus ok` (owned by v20e)

## Touched paths (allowlist)

- `source/services/a11yd/` (extend)
- `userspace/ui/kit/` (a11y attributes + focus order hints)
- SystemUI focus wiring
- `tests/ui_v20a_host/`
- `docs/a11y/overview.md` (new/extend)

## Plan (small PRs)

1. a11yd IDL hardening + markers
2. adapters (SystemUI + kit) + action routing
3. focus nav + tests + docs
