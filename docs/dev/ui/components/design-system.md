<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Design System Components

This document defines the **component-facing** design system contract:

- standard reusable controls/primitives,
- their basic semantics and state model,
- and how they relate to foundations, shell patterns, and accessibility.

This document is intentionally narrower than the full UI information architecture.

Use these category indexes alongside it:

- Foundations: `docs/dev/ui/foundations/`
- Patterns: `docs/dev/ui/patterns/`
- Components: `docs/dev/ui/components/`
- Input: `docs/dev/ui/input/`
- Presentation: `docs/dev/ui/presentation/`

## Component posture

- Components should stay small, stable, and reusable across apps and SystemUI.
- Components are **not** the place to encode whole app-shell/windowing decisions.
- Navigation shells (sidebar/tabbar/inspector/snap) belong primarily in Patterns and Navigation docs, even if they reuse
  component primitives internally.
- Motion, typography, colors, materials, and scaling rules belong to Foundations and are referenced here rather than
  duplicated.

## Component examples (illustrative)

```nx
Button {
  label: "Continue"
  kind: Primary
  on Tap -> emit(AppEvent::Continue)
}
```

```nx
TextField {
  value: $state.email
  placeholder: "name@example.com"
  on Change(text) -> emit(AuthEvent::EmailChanged(text))
}
```

## Standard components (v0.x)

Open Nexus OS intends to ship a small, stable set of “standard primitives” (ArkUI/iOS-like in spirit) that SystemUI and apps can rely on.

Planned initial set (see `tasks/TASK-0073-ui-v10a-design-system-primitives-goldens.md`):

- **Inputs**: Button, IconButton, TextField, Checkbox, Switch, Slider
- **Structure**: ListRow, Card, Divider
- **Overlays**: Dialog, Sheet, ToastView (visual widget; modal manager is tracked separately)

In the DSL, these appear as first-class view nodes (with deterministic goldens and a11y semantics), and the underlying implementation should remain consistent with the UI kit primitives.

## Near-boundary components

Some surfaces are visually component-like, but sit near a larger category boundary:

- **Segmented control**:
  - treated as a reusable component when used for small in-page mode switches
- **Tabs / tab bars**:
  - the control itself is component-like, but top-level app usage belongs in Navigation
- **Sheets / dialogs / toast**:
  - the visible widgets are components, but lifecycle/showing rules belong in Presentation
- **Search bars / path controls / sidebars**:
  - generally documented under Navigation or Patterns rather than as isolated components

When in doubt:

- put reusable visual/state semantics here,
- put shell/layout/placement/lifecycle contracts in the relevant category index.

## Related

- Components index: `docs/dev/ui/components/`
- Goldens: `docs/dev/ui/foundations/quality/goldens.md`
- Theme/tokens: `docs/dev/ui/foundations/visual/theme.md`
- Colors: `docs/dev/ui/foundations/visual/colors.md`
- Typography: `docs/dev/ui/foundations/visual/typography.md`
- Curated font library: `docs/dev/ui/foundations/visual/font-library.md`
- Icons: `docs/dev/ui/foundations/visual/icons.md`
- Icon design guidelines: `docs/dev/ui/foundations/visual/icon-guidelines.md`
- Display scaling: `docs/dev/ui/foundations/layout/display-scaling.md`
- App shell patterns: `docs/dev/ui/patterns/app-shell-patterns.md`
- Glass materials: `docs/dev/ui/foundations/visual/materials.md`
