<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Design System

This document defines the **developer-facing** design system contract:

- core components (Button/TextField/List/Modal…),
- layout/spacing primitives,
- typography and motion guidance,
- accessibility semantics,
- and how the system behaves across phone/tablet/desktop.

## Foundation rules (opinionated)

- **One design language** across devices (desktop does not become “ugly/complex”).
- **Command surface > menu bar**:
  - provide a command palette / search-first actions,
  - prefer contextual top bars and inline actions,
  - avoid “FILE / EDIT / VIEW / HELP” as a default pattern.
- **Touch targets stay reasonable** even on desktop; pointer affordances are additive (hover/focus/shortcuts).

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

## Navigation components (Tabs, segmented, sidebars)

Navigation patterns exist, but they span **app shell + windowing**, so we treat them as “recommended shell patterns” rather than purely visual primitives.

Guidance:

- **Tabs (phone-first)**:
  - use a **bottom tab bar** on phone where it makes sense (few top-level sections)
  - avoid too many tabs; keep labels/icons clear and consistent
- **Segmented control (tablet/desktop)**:
  - use a segmented control for “small mode switches” within a page (not for deep navigation)
- **Sidebar / inspector (desktop/tablet)**:
  - prefer the recommended zone model (LeftSidebar + Content + optional RightInspector)

See:

- Recommended shell patterns: `docs/dev/ui/app-shell-patterns.md`
- Windowing/snap: `docs/dev/ui/wm.md`, `docs/dev/ui/wm-snap.md`

## Related

- Goldens: `docs/dev/ui/goldens.md`
- Theme/tokens: `docs/dev/ui/theme.md`
- Colors: `docs/dev/ui/colors.md`
- Typography: `docs/dev/ui/typography.md`
- Curated font library: `docs/dev/ui/font-library.md`
- Icons: `docs/dev/ui/icons.md`
- Icon design guidelines: `docs/dev/ui/icon-guidelines.md`
- Display scaling: `docs/dev/ui/display-scaling.md`
- App shell patterns: `docs/dev/ui/app-shell-patterns.md`
- Glass materials: `docs/dev/ui/materials-glass.md`
