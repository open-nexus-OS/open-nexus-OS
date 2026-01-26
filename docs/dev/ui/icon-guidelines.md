<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Icon Design Guidelines

This document defines **how icons are designed, packaged, themed, and used** in Open Nexus OS.

We use Apple’s icon guidance as a reference for clarity and rigor (not as a style mandate): see [Apple HIG – Icons](https://developer.apple.com/design/human-interface-guidelines/icons).

## Goals

- **SVG-first**: avoid “30 bitmap files” per icon and scale class.
- **Theming**: users can choose icon themes; apps can optionally provide a **symbolic** icon that the system can recolor.
- **Determinism**: rendering must be stable for goldens (no nondeterministic filters/fonts).
- **Compatibility**: align with **freedesktop/XDG icon naming + theme layout** to reuse existing icon themes.

## Icon types (two-tier model)

### 1) System/UI icons (in-app chrome)

- Used for buttons, menus, toolbars, status indicators.
- Default set: **Lucide** (see `docs/dev/ui/icons.md` for sizes/strokes).
- **Always treat these as symbolic** (monochrome) at render-time (color comes from tokens).

### 2) App icons (identity)

Apps should provide:

- **Brand icon** (full color): for launcher/store surfaces where brand identity matters.
- **Symbolic icon** (template): monochrome SVG suitable for user theming (e.g., “all black icons on transparent background”).

If an app provides only one asset, treat it as the **brand icon** and do not attempt to recolor it.

## SVG-only policy (and the allowed subset)

We require **SVG** for UI icons and app icons.

To keep rendering deterministic and predictable:

- **Allowed**: paths and simple shapes (e.g. `path`, `rect`, `circle`, `line`, `polyline`, `polygon`), basic `transform`, `stroke`, `fill`.
- **Disallow**: external images, embedded fonts/text, filters, blur, feGaussian*, feTurbulence, complex masks that are renderer-dependent.
- **No hardcoded colors** for symbolic icons: use a single color channel (e.g. `currentColor`) and transparency only.

## Grid, sizes, and strokes

### Canonical grid

- Source assets should use a canonical **24×24 viewBox** where possible.
- Keep consistent “visual padding” so icons feel aligned even when shapes differ.

### Sizes & stroke tokens

Use the canonical sizes/strokes from `docs/dev/ui/icons.md`:

- Sizes: **16 / 20 / 24 (default) / 32**
- Strokes: **1.5 / 1.75 / 2.0**

### Pixel snapping (crispness)

- Prefer integer pixel sizes and snap placement to whole pixels (see `docs/dev/ui/display-scaling.md`).
- Thin dividers/lines must be placed so they land cleanly at 1.0x and 2.0x.

## freedesktop/XDG compatibility

We should be compatible with:

- **Icon theme structure** (theme inheritance, `hicolor` fallback)
- **Icon naming** conventions (so third-party themes “drop in”)

Practical guidance:

- Store themes using the freedesktop-style folder layout (including `scalable/` where applicable).
- Maintain a small **alias mapping** from our internal “semantic icon IDs” to freedesktop names (e.g. internal `app.close` → `window-close`).

## Using icons from the DSL/UI layer

Expose icons in the DSL as **semantic names**, not file paths:

- Example mental model: `Icon("search", size: 24)` or `Icon("app.close")`

Resolution order (recommended):

1. App-provided icon (when referencing an app asset)
2. Current icon theme (freedesktop name)
3. Built-in fallback set (Lucide)

## Folder icons (template + dynamic emblem overlay)

Goal: avoid shipping many folder SVG variants while still supporting user customization (folder color + per-folder emblem), similar to modern desktop systems.

### Folder template asset

- Provide **one canonical folder template** SVG per style family (keep this small; ideally 1).
- The folder template must be **recolorable**:
  - use tokenized fills/strokes (or `currentColor` + named layers) rather than hardcoded colors
  - keep shapes simple (allowed SVG subset only)
- Do not bake in an emblem; emblems are composed at render-time.

### Emblem (overlay icon)

- Emblems are **symbolic icons** (monochrome SVG) from:
  - the current icon theme (freedesktop name), or
  - the built-in fallback set (Lucide).
- Emblem placement must be one of a **small set of canonical placements** (tokens):
  - **Front-center** (primary, “Finder-like”)
  - **Bottom-right badge** (optional, good for status overlays)
- Emblem size must be chosen from canonical icon sizes (16/20/24/32).

### “Darker shadow on top” look (deterministic)

Do not use SVG filters/blur to achieve the embossed look. Instead:

- Derive an emblem color from the folder base color:
  - Example: `emblemColor = darken(folderColor, 0.35)` (exact function is a renderer/token contract)
  - Apply a stable alpha (e.g. 0.85)
- Optional: a second “inset” pass (1px offset) using a slightly different alpha to hint depth (still no blur).

This produces a consistent “shadowed emblem” feel across devices and keeps goldens deterministic.

### Data model (recommended)

Persist per-folder appearance as data, not as generated SVG files:

- `folderColor` (token or explicit color)
- `emblemIconId` (semantic name / freedesktop name)
- `emblemStyle` (`Embossed` | `Badge` | `None`)
- `placement` (`FrontCenter` | `BottomRight`)

### Small-size legibility

At 16px, some emblems become unreadable. Options:

- restrict the emblem set for small sizes (“emblem-safe” list), or
- allow an optional `*.micro.svg` emblem variant for 16px.

## Checklist (review)

- **Shape language**: consistent corner/radius feel; no random geometry.
- **Optical alignment**: baseline/centering feels right in lists and toolbars.
- **Small-size legibility**: ensure it still reads at 16px.
- **Symbolic theming**: symbolic icon has no baked-in colors and looks good when recolored.
- **Determinism**: uses only the allowed SVG subset.
