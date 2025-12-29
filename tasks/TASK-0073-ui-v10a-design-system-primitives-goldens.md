---
title: TASK-0073 UI v10a (host-first): Design System v1 façade + core primitives + snapshot goldens + a11y/contrast lints
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Theme tokens baseline: tasks/TASK-0063-ui-v5b-virtualized-list-theme-tokens.md
  - Layout/wrapping baseline: tasks/TASK-0058-ui-v3a-layout-wrapping-deterministic.md
  - Input/a11y baseline: tasks/TASK-0061-ui-v4b-gestures-a11y-semantics.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

UI functionality exists across SystemUI and apps, but visuals and interaction semantics are inconsistent.
We need a small, stable “design kit” that:

- derives from existing theme tokens,
- standardizes states (hover/pressed/disabled/focus),
- provides snapshot goldens for visual regressions,
- and runs deterministic host tests.

This task is **host-first**. App shell and adoption/migration is `TASK-0074`.

## Goal

Deliver:

1. `userspace/ui/design`:
   - façade over tokens → spacing/radius/typography/motion/colors
   - motion presets (fast/normal/slow + standard easings)
   - subscribes to theme changes (signal-based) for live updates
2. `userspace/ui/kit` primitives (initial set):
   - Button, IconButton, TextField, Checkbox, Switch, Slider
   - ListRow, Card, Divider
   - Dialog, Sheet, ToastView (visual widgets; modal manager is v10b)
   - all primitives emit A11y roles/labels (where applicable)
3. Snapshot golden harness:
   - render each primitive in states (default/hover/pressed/disabled) in light/dark
   - compare PNGs (pixel-exact preferred; SSIM threshold if required and documented)
4. A11y lints in tests:
   - minimum touch target size checks
   - contrast checks against configurable threshold (WCAG AA style)

## Non-Goals

- Kernel changes.
- Full app shell / chrome (v10b).
- Migration of SystemUI/apps (v10b).

## Constraints / invariants (hard requirements)

- Deterministic rendering and tests:
  - stable rasterization parameters,
  - stable layout and rounding rules,
  - explicit SSIM thresholds if pixel-exact is not portable.
- Bounded memory for widget caches (glyph atlas already exists elsewhere; primitives must not grow unbounded).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`cargo test -p ui_v10_goldens` green:

- goldens for core primitives in light/dark and key states
- a11y lints:
  - touch targets meet minimum
  - contrast meets threshold (configurable)

## Touched paths (allowlist)

- `userspace/ui/design/` (new)
- `userspace/ui/kit/` (new)
- `tests/ui_v10_goldens/` (new)
- `tools/gen-goldens.sh` (new helper, optional)
- `docs/ui/design-system.md` + `docs/ui/goldens.md` (new)

## Plan (small PRs)

1. design façade (spacing/radius/type/motion/colors)
2. core primitives (button/textfield/checkbox/switch/slider)
3. extended primitives (listrow/card/dialog/sheet/toast view)
4. goldens harness + a11y lints + docs
