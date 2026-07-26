<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Colors (Tokens, Palettes, Themes)

This document defines the **color system contract** for Open Nexus OS UI.

Goals:

- Keep the UI **modern and friendly** without letting users pick arbitrary “bad” colors.
- Make apps **follow the OS theme by default** via semantic tokens (`accent`, `bg`, `fg`, ...).
- Provide a small set of **curated presets** (curated in spirit, without copying any vendor palette).

## Principles

- Prefer **semantic tokens** over raw colors in UI code.
- User customization is **bounded**:
  - choose from a curated **accent palette**
  - optionally choose from a small set of theme presets
  - avoid a full free-form color picker as the default.

## Semantic color tokens (v0.x)

These are the tokens UI components and DSL modifiers should use:

- `bg` / `fg`
- `surface` / `surfaceAlt`
- `border` / `divider`
- `muted` / `mutedFg`
- `accent` / `accentFg`
- `danger` / `dangerFg`
- `warning` / `warningFg`
- `success` / `successFg`
- `focusRing`

Notes:

- `accent` is the primary “personality” color. Apps should use it for primary actions unless they opt out.
- `*Fg` tokens must meet contrast requirements against their paired background token.

### On glass and on the wallpaper (RFC-0082)

A second family exists because some surfaces sit on a **photograph**, not on
`surface`. `onSurface` is tuned for a solid panel and disappears against a
bright sky, so the lock screen, the dock, the launcher and the control center
read through these instead:

| Token | Use |
|---|---|
| `onGlass` / `onGlassMuted` / `onGlassStrong` | text on glass or wallpaper (primary / secondary / high-emphasis) |
| `glassIcon` | icon stroke on glass |
| `glassPlaceholder` | placeholder text in a glass field |
| `glassFocus` | border of a FOCUSED glass control (a tint of the ink, not the solid `focusRing` blue) |
| `glassFill` | flat translucent fill of a control sitting INSIDE glass — never its own glass layer, the compositor does not nest backdrop blur |
| `wallpaperTint` | full-bleed wash that keeps content legible without killing the image |
| `wallpaperVignette` | bottom stop of the legibility fade (pair with `transparent` via `.bgFade`) |
| `textShadow` / `textShadowStrong` | the two `.textShadow()` tints — a light halo in light mode, a drop shadow in dark |
| `transparent` | the identity color; a gradient stop |

These flip direction between themes in a way the surface tokens do not: in
light mode the text shadow is **white** (dark ink needs a bright bloom to
separate), in dark mode black.

## Curated accent palette

User-facing accent selection should be from a small set of “always looks OK” options.

Recommended palette (illustrative names; actual hex values live in the theme):

- `accent.blue` (default)
- `accent.indigo`
- `accent.purple`
- `accent.pink`
- `accent.red`
- `accent.orange`
- `accent.yellow`
- `accent.green`
- `accent.teal`

Rules:

- Keep the palette small (≈ 8–12).
- Each accent must ship with a tested `accentFg` pairing.
- Accents must be consistent across light/dark themes.

## Tag colors (Finder-style labels)

File tags/labels should use the same curated palette for consistency:

- Tag color is a **palette ID** (not an arbitrary RGB value).
- Rendering uses theme tokens derived from that palette ID (consistent across light/dark).

## Theme presets (Settings)

The system should offer a small set of presets instead of full custom theming:

- `Light`
- `Dark`
- `Midnight` (deeper dark)
- `Warm` (slightly warmer neutrals)
- `HighContrast`

Users may select:

- one preset
- one accent from the curated palette

Apps may:

- follow the system entirely (default)
- override a **subset** of tokens (advanced; still must remain bounded and contrast-safe).

## DSL guidance (modifiers)

In DSL/UI code:

- **Use tokens**: `bg(accent)`, `fg(onSurface)`, `borderColor(divider)`, ...
- Avoid raw colors in v0.x UI code unless explicitly whitelisted (theme authoring is where raw values belong).
- An unknown token is a **compile error** — `.fg(oNSurface)` used to type-check
  and then silently paint the default (RFC-0082).
- On a surface that sits on the wallpaper, reach for the on-glass family above
  rather than `onSurface`/`onSurfaceVariant`.
