---
title: TASK-0058 UI v3a: deterministic layout engine (flex/grid/stack) + text wrapping + host goldens
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - UI v2b shaping baseline: tasks/TASK-0057-ui-v2b-text-shaping-svg-pipeline.md
  - UI v2a present/input baseline: tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md
  - Drivers/Accelerators contracts (buffers/sync/QoS): tasks/TRACK-DRIVERS-ACCELERATORS.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

UI v3 introduces “real UI structure”: deterministic layout, text wrapping, and stable measurement contracts.
This must be **deterministic** and **testable headlessly** (host-first), so later windowd/plugins can
use it without layout drift.

This task is **v3a** (layout + wrapping). Clipping/scroll/effects/IME are deferred to v3b (`TASK-0059`).

## Goal

Deliver:

1. `userspace/ui/layout` crate:
   - Flex (row/column), Stack (relative/absolute), Grid v1 (fraction columns)
   - stable ordering and stable box outputs
   - deterministic numeric rules (integer or fixed-point)
2. Text measurement bridge:
   - `MeasureText` callback that integrates with the shaper/wrapper
3. Wrapping helpers in `userspace/ui/shape`:
   - Unicode line breaking (minimal UAX#14 subset)
   - ellipsis and max-lines truncation
4. Host tests:
   - layout JSON goldens
   - wrapping JSON goldens (+ optional PNG goldens for rendered output)
5. Markers for OS bring-up later:
   - `layout: engine on`
   - `text: wrapping on`

## Non-Goals

- Kernel changes.
- Scroll, clipping, effects, IME/text input (v3b).
- Full CSS or complete grid auto-placement.

## Constraints / invariants (hard requirements)

- Deterministic output:
  - no floating-point drift; use fixed-point or integer rounding rules that are documented.
  - stable traversal order.
- Bounded compute:
  - cap node count per layout call,
  - cap recursion depth.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **YELLOW (UAX#14 completeness)**:
  - v3a can implement a minimal line-breaking subset sufficient for deterministic wrapping,
    but must document what is unsupported (hyphenation, complex line break classes) to avoid drift.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v3a_host/`:

- layout:
  - style trees → stable box outputs (JSON goldens)
  - flex grow/shrink edge cases
  - grid fraction sizing and gaps
- wrapping:
  - multilingual samples produce stable line break points and advance sums (JSON goldens)
  - ellipsis and max-lines truncation rules verified

### Proof (OS/QEMU) — optional/gated

Once windowd/plugins consume layout:

- `layout: engine on`
- `text: wrapping on`
- `SELFTEST: ui v3 wrap ok` (added in v3b integration task)

## Touched paths (allowlist)

- `userspace/ui/layout/` (new)
- `userspace/ui/shape/` (extend: wrapping)
- `userspace/ui/renderer/` (optional: draw_wrapped_text helper)
- `tests/ui_v3a_host/` (new)
- `docs/dev/ui/layout.md` + `docs/dev/ui/wrapping.md` (new)

## Plan (small PRs)

1. **Layout crate**
   - node/style model, measurement callback
   - flex/stack/grid v1 algorithms
   - deterministic numeric handling

2. **Wrapping**
   - minimal line break opportunities + truncation/ellipsis
   - measurement integration with shaping outputs

3. **Tests**
   - JSON goldens for layout and wrapping
   - optional small PNG checks for rendered wrapped text (if stable)

4. **Docs**
   - determinism rules and unsupported features
