<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# UI Foundations

Foundations define the system-wide rules that every UI surface builds on.

Use this category for:

- accessibility semantics, focus, reduced-motion, and visibility rules,
- color and theme tokens,
- typography and font posture,
- icons and scaling,
- motion and transitions,
- materials/effects,
- layout/runtime/rendering contracts,
- and performance/testing expectations.

Current entry points:

- `docs/dev/ui/foundations/development/README.md`
- `docs/dev/ui/foundations/accessibility/README.md`
- `docs/dev/ui/foundations/visual/README.md`
- `docs/dev/ui/foundations/motion/README.md`
- `docs/dev/ui/foundations/layout/README.md`
- `docs/dev/ui/foundations/rendering/README.md`
- `docs/dev/ui/foundations/quality/README.md`
- `docs/dev/ui/foundations/visual/colors.md`
- `docs/dev/ui/foundations/visual/theme.md`
- `docs/dev/ui/foundations/visual/typography.md`
- `docs/dev/ui/foundations/visual/font-library.md`
- `docs/dev/ui/foundations/visual/icons.md`
- `docs/dev/ui/foundations/visual/icon-guidelines.md`
- `docs/dev/ui/foundations/layout/display-scaling.md`
- `docs/dev/ui/foundations/animation.md`
- `docs/dev/ui/foundations/transitions.md`
- `docs/dev/ui/foundations/layout/layout.md`
- `docs/dev/ui/foundations/layout/layout-pipeline.md`
- `docs/dev/ui/foundations/layout/text.md`
- `docs/dev/ui/foundations/rendering/runtime.md`
- `docs/dev/ui/foundations/rendering/renderer.md`
- `docs/dev/ui/foundations/quality/performance-philosophy.md`
- `docs/dev/ui/foundations/visual/materials.md`
- `docs/dev/ui/foundations/quality/testing.md`
- `docs/dev/ui/foundations/quality/goldens.md`

Rule of thumb:

- if a rule should apply to Files, Browser, Settings, SystemUI, and third-party apps alike, it probably belongs in
  Foundations.
- if a rule explains how visible developer surfaces like Console, Package Manager, or Dev Studio should behave together,
  it belongs in `docs/dev/ui/foundations/development/`.
