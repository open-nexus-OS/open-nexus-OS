<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# UI Components

Components are the standard reusable controls and visual primitives used by apps and SystemUI.

Use this category for:

- buttons and form controls,
- rows/cards/dividers,
- reusable overlay widgets,
- simple status widgets,
- and stable component semantics backed by goldens and a11y rules.

Current entry points:

- `docs/dev/ui/components/actions/README.md`
- `docs/dev/ui/components/containers/README.md`
- `docs/dev/ui/components/navigation/README.md`
- `docs/dev/ui/components/input-and-selection/README.md`
- `docs/dev/ui/components/status-and-feedback/README.md`
- `docs/dev/ui/components/media-and-content/README.md`
- `docs/dev/ui/components/design-system.md`
- `docs/dev/ui/components/inventory.md` — IST state of all 54 handoff components across the three
  UI expressions + the "promote the best" convergence verdict per component
- `docs/dev/ui/foundations/visual/token-reconciliation.md` — the four token sources → one SSOT
- `docs/dev/ui/input/input.md`
- `docs/dev/ui/foundations/visual/typography.md`
- `docs/dev/ui/status/notifications.md`

Architecture spine:

- `docs/rfcs/RFC-0070-ui-design-system-ssot-convergence.md` — one declarative component library,
  one token source, one glass primitive, one reactive path (`LayoutNode → LayoutEngine →
  SceneGraph → nexus-gfx`); the design contract is `docs/dev/design_handoff_open_nexus_os/`.

Implementation/task anchor:

- `tasks/TASK-0073-ui-v10a-design-system-primitives-goldens.md` (primitives, host-first)
- `tasks/TASK-0074-ui-v10b-app-shell-adoption-modals.md` (overlays/modals/App Shell + windowd convergence)

Boundary note:

- shell/navigation/windowing rules live in Patterns or Navigation,
- heavy render/input cores live in Blessed Surfaces,
- and component appearance/motion tokens live in Foundations.
