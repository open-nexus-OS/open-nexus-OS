<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# App Shell Patterns (Recommended)

This page describes the **recommended** app shell, layering, and windowing patterns for Open Nexus OS.

It is guidance, not a hard ABI: apps may deviate, but should keep the same design language and remain cross-device.

See also:

- App shell overview: `docs/dev/ui/app-shell.md`
- Profiles: `docs/dev/ui/profiles.md`
- Windowing: `docs/dev/ui/wm.md`, `docs/dev/ui/wm-snap.md`

## Goals

- **Consistency**: apps feel like they belong to one system.
- **Cross-device**: the same surfaces scale from phone → tablet → desktop → TV.
- **Discoverability**: primary actions and search remain easy to find.
- **Flexibility**: the look/material can evolve (e.g. “glass” treatments) without breaking layout contracts.

## Layout zones (role-based, not pixel-based)

Think in zones/roles instead of fixed pixels:

- **TopBar**: global actions + search + app-level context
- **LeftSidebar / Rail**: navigation (collections, roots, spaces)
- **Content**: the primary document/canvas/list
- **RightInspector**: properties/inspector panel (Figma-style)
- **BottomBar / Status** (optional): background tasks, progress, status

Rules:

- Zones may **collapse** into drawers on smaller screens, but the roles stay the same.
- The **Content** zone is primary; side panels should not starve it on small devices.

## Recommended default shell (desktop)

### Window chrome and controls

- Window controls (minimize / maximize / close) are **system chrome**.
- On desktop, they are placed **top-right** (system-controlled, not app-provided).

### Top bar

We prefer a top bar over a legacy menu bar:

- Primary actions and context live in the TopBar.
- Search may live in the TopBar (especially for “files/settings/search-first” surfaces).
- The TopBar can host mode toggles (list/grid), breadcrumb context, and scoped actions.

### Sidebar + inspector (split pattern)

Recommended structure for “pro” surfaces:

- LeftSidebar for navigation
- RightInspector for properties
- Content in the middle

Guidance:

- Both side panels should support **collapse** and **remembered visibility** per app (bounded persistence).
- Inspector content must be structured and searchable when it grows.

## Tablet windowing (drag handle + remembered states)

Tablet profile should support both fullscreen and windowed modes without becoming “desktop only”.

### Drag handle (“dragbar”)

- A small top handle can be shown to enable:
  - pulling an app into windowed mode,
  - drag-to-snap interactions.

### Remembered window state

We should remember (per app/session):

- whether the app was fullscreen vs windowed,
- last snap configuration (if any),
- last visible zones (sidebar/inspector collapsed).

This memory must be bounded and deterministic (no unbounded per-folder/per-window state growth).

### Snap + auto layout

Snap rules should be predictable and device-profile aware:

- snap zones are canonical (left / right / thirds, depending on device)
- minimum sizes are respected
- transitions are deterministic (stable placements, no “random” easing surprises)

## Layering and “glass” style (non-binding)

We may adopt layered materials (e.g. translucent surfaces) in the future.

Contract:

- Material choices must not change the zone model.
- Effects must remain deterministic enough for goldens (avoid renderer-dependent blur where it breaks reproducibility).
- Prefer token-driven materials (surface elevation levels) so themes can evolve.

See also: `docs/dev/ui/materials-glass.md` (cheap glass recipe + update policy + reduce-transparency).

## Extensibility (allow other schemas)

We recommend the default shell above, but allow alternative schemas if they:

- preserve the zone roles (TopBar/Sidebar/Content/Inspector),
- keep primary actions and search discoverable,
- remain cross-device and deterministic.
