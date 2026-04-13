---
title: TASK-0055C UI v1d: windowd visible present + SystemUI first frame in QEMU
status: Draft
owner: @ui
created: 2026-03-28
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Visible scanout bootstrap: tasks/TASK-0055B-ui-v1c-visible-qemu-scanout-bootstrap.md
  - UI v1b compositor baseline: tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md
  - Renderer wiring: tasks/TASK-0170-renderer-abstraction-v1b-os-windowd-wiring-textshape-perf-markers.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Once the system can show a guest-visible framebuffer, the next missing step is to make the **real `windowd` output**
land on that surface. This task converts the invisible/headless present path into the first visible UI frame.

It is the bridge from:

- "display exists, but only shows a pattern"

to

- "`windowd`/SystemUI draw something real and visible."

## Goal

Deliver:

1. Visible `windowd` present path:
   - the same frame built by `windowd` for headless present is written to the visible display target
   - full-frame present is acceptable in v1d; dirty-rect optimization is a follow-up
2. Minimal SystemUI visible shell frame:
   - draw a deterministic desktop background and a minimal shell surface
   - no launcher interaction required yet; this task only proves that the shell frame is visible
3. Marker and visual parity:
   - the visible present reuses the same present lifecycle as the headless path
   - UART markers remain stable and bounded

## Non-Goals

- Rich shell UI.
- Cursor or pointer rendering.
- Input routing.
- Window management beyond a minimal first frame.
- Quick Settings, Notifications, or app launching.

## Constraints / invariants (hard requirements)

- No parallel "debug renderer"; use the same `windowd` composition path.
- The visible path must not bypass `renderer::Backend`.
- Markers must correspond to real visible present, not just a headless compose.
- Deterministic frame contents and deterministic shell background.

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — required

UART markers:

- `windowd: backend=visible`
- `windowd: present visible ok`
- `systemui: first frame visible`
- `SELFTEST: ui visible present ok`

Visual proof:

- QEMU graphics window shows a deterministic shell/background frame sourced from `windowd`

## Touched paths (allowlist)

- `source/services/windowd/`
- SystemUI bootstrap frame path
- display bootstrap service integration
- `source/apps/selftest-client/`
- `docs/dev/ui/overview.md`
- `docs/dev/ui/foundations/quality/testing.md`

## Plan (small PRs)

1. `windowd` visible backend handoff
2. minimal SystemUI first frame
3. markers + selftests + docs
