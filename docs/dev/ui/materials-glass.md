<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Materials: “Glass” (Resource-Conscious)

This page defines the recommended **translucent (“glass”) material system** for Open Nexus OS UI.

Goal: achieve a modern layered look (floating sidebars, control center, sheets) without requiring
expensive “real glass” rendering.

This is guidance, not a hard ABI: the exact look may evolve, but the **tokens, update policy, and fallbacks**
should remain stable.

## Design intent (what “glass” means here)

Our “glass” is a **frosted layer**:

- a blurred snapshot of the content behind the surface (“backdrop”),
- a subtle white/black tint so foreground content stays legible,
- and a small edge highlight so it reads as a distinct layer.

We do **not** aim for physically correct refraction or per-pixel distortion.

## Where we use glass (recommended)

Primary:

- **Floating left sidebar** (desktop/tablet; “new layered sidebar”)
- **Control Center** / quick settings overlay
- **Popovers, menus, toasts** (where appropriate)

Secondary (subtle):

- window chrome / top bars (optional, low translucency)

Avoid:

- large continuous backgrounds behind text-heavy content (can hurt readability and perf).

## Material tokens

Treat materials as theme tokens (authoring in `*.nxtheme.toml`):

- `material.surface` (opaque)
- `material.glassLow` (subtle translucency)
- `material.glassHigh` (more pronounced translucency)

Each glass material defines:

- `blurRadiusDp` (logical blur radius)
- `downsampleFactor` (e.g. 2, 4, 8)
- `tintColor` + `tintAlpha` (usually white/black with low opacity)
- `edgeHighlightColor` + `edgeHighlightAlpha`
- `borderColor` + `borderAlpha` (optional)

## Rendering recipe (cheap glass)

For a glass surface:

1. Capture a **backdrop snapshot** of the scene region behind the surface.
2. Downsample it (`downsampleFactor`) and apply a **separable blur** (fast).
3. Composite:
   - blurred backdrop
   - tint overlay (white/black, low opacity)
   - optional subtle noise (only if deterministic and cheap)
   - 1–2px edge highlight / inner stroke

Important:

- Prefer **clip + caching** per surface (sidebar/control center), not per widget.
- Avoid filters that are renderer/hardware dependent in ways that break goldens.

## Update policy (live only when it matters)

We want the glass to feel “attached to the world” while moving, but be cheap while idle.

### Backdrop refresh triggers

Refresh the backdrop when:

- the glass surface is **animating** (opening/closing, dragging, resizing),
- the **background content** behind it changes (damage/dirty rect intersects the backdrop region),
- the window stack changes (window moved underneath, new overlay appears).

### Throttling and degradation

If the background is “hot” (e.g. video behind the control center):

- throttle backdrop refresh to a bounded rate (e.g. 30Hz or lower),
- or degrade gracefully:
  - keep tint + edge highlight,
  - reduce blur quality (higher downsample),
  - or temporarily switch to `material.surface` (opaque) in low-power mode.

### Idle behavior

If neither the surface nor the background is changing:

- keep the cached backdrop (“frozen glass”),
- and only animate foreground UI (cheap).

## Accessibility & user preference

Provide a system setting:

- **Reduce transparency** / **Solid surfaces**

When enabled:

- disable backdrop blur and translucency,
- use `material.surface` + border (still layered, but opaque),
- keep all content contrast-safe.

This setting should also be used as a low-power fallback path.

## Determinism requirements (goldens)

- The blur algorithm and parameters must be stable and deterministic.
- If noise is used, it must be deterministic (seeded/patterned) and not frame-random.
- Material selection must be token-driven and testable.

## Related

- Colors/tokens: `docs/dev/ui/colors.md`, `docs/dev/ui/theme.md`
- App shell zones (sidebar/control center surfaces): `docs/dev/ui/app-shell-patterns.md`
- Compositor concepts: `docs/dev/ui/compositor.md`
