<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Colors (Tokens, Palettes, Themes)

This document defines the **color system contract** for Open Nexus OS UI.

Goals:

- Keep the UI **modern and friendly** without letting users pick arbitrary “bad” colors.
- Make apps **follow the OS theme by default** via semantic roles (`label`, `accent`, `fill`, ...).
- Provide a small set of **curated presets** (curated in spirit, without copying any vendor palette).

## Principles

- Prefer **semantic tokens** over raw colors in UI code.
- User customization is **bounded**:
  - choose from a curated **accent palette**
  - optionally choose from a small set of theme presets
  - avoid a full free-form color picker as the default.

## Semantic colour roles

Colours are roles, never swatches. The set is the design artifacts' 31 roles plus
`wallpaperTint`, `labelShadow` and `transparent` (RFC-0116) — `ColorToken` in
`userspace/ui/theme-tokens/src/color_token.rs`, whose names are the theme keys
(`resources/themes/*.nxtheme.toml` `[tokens]`) and the DSL names. Every role resolves in light,
dark and high contrast; the typed tokens' build fails on a theme-varying role a variant does
not author.

| Role | Use |
|---|---|
| `background` | the opaque page area behind content that is not on the wallpaper |
| `label`, `labelSecondary`, `labelTertiary` | text and icons: content · metadata, section labels, inactive icons · placeholders and disabled only |
| `separator` | every hairline (dividers, list separators, pane and chip borders) |
| `fill` | the resting fill of a control on a surface (switch-off track, segment, slider track, field, chip, secondary button) |
| `fillHover`, `fillActive` | the hover and press layer — shown by opacity, never set as a background |
| `glassPanel`, `glassWindow` | the two blur surfaces: panels, the dock, launcher sheets · app windows |
| `glassCard` | a translucent layer on a blur surface (cards, grouped rows, tiles); never blurs |
| `glassOverlay` | a floating surface's frosted glass (modal, alert, menu, popover, toast, select list, action bar): four fifths opaque over the regular blur — `.material(overlay)` as a root |
| `glassEdge` | the 1 px light edge of every glass layer |
| `surfacePane` | the opaque pane inside a window |
| `surfaceOverlay` | the opaque floating surface: the on-screen keyboard's tray, and what a floating surface must not show through |
| `scrim` | the dimming layer behind a modal overlay |
| `primary`, `onPrimary` | the brand's solid button and its label |
| `accent`, `accentGlow`, `onAccent` | the one tint (on-states, selection, links, focus) · its glow · text on it and on `danger` |
| `danger`, `success`, `warning` | status |
| `modeAirplane`, `modeDesktop`, `modeTablet` | the mode toggles' on-states |
| `island` | the activity runner's background |
| `chart1` … `chart5` | chart series |
| `wallpaperTint` | the wash over the wallpaper that keeps content legible (white 20 % light, black 35 % dark) |
| `labelShadow` | the halo behind a label on the wallpaper (a light glow in light, a dark one in dark) |
| `transparent` | no colour (a gradient stop, a hidden edge) |

Notes:

- `accent` is the “personality” colour — the one tint for on-states, selection, links and
  focus, and the one a user picks (below); the brand's solid button is `primary`.
- A tinted status background is the role at a fixed alpha (`.tint(role)`: 16 %, 30 % or 55 %),
  never a role of its own; the hover and press layers on a coloured fill are constants of that
  layer, not roles.
- The `on*` roles must meet contrast requirements against the role they sit on.

### On the wallpaper

A surface on a photograph reads through the same label roles: the material's fill
(`glassPanel`, `glassWindow`, `glassCard`) keeps the label legible. A label sitting directly on
the wallpaper (the top bar, an icon label) takes `.shadow(halo)` — a soft `labelShadow` glow:
the glyphs blurred 3 px, once at the text and once 1 px lower at four fifths of the alpha, as
the artifacts draw it — and `wallpaperTint` is the full-bleed wash that keeps
content legible without killing the image. `labelShadow` flips between themes in a way the
surface roles do not: in light mode it is **white** (dark ink needs a bright bloom to
separate), in dark mode black.

## Curated accent palette

User-facing accent selection should be from a small set of “always looks OK” options.

The palette (`ACCENT_PALETTE` in `userspace/ui/theme-tokens/src/lib.rs`, a light and a dark
value each; `default` is the theme's own blue):

- `default` (blue)
- `violet`, `pink`, `red`, `orange`, `green`, `teal`, `amber`, `graphite`

Rules:

- Keep the palette small (≈ 8–12).
- A user accent moves `accent` and `accentGlow` together (`AccentTokens`: an orange accent
  glows orange); `onAccent` must stay legible on every entry.
- Accents must be consistent across light/dark themes.

## Tag colors (file labels)

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

- **Use roles**: `.bg(fill)`, `.fg(labelSecondary)`, `.borderColor(separator)`,
  `.tint(accent)` — the catalog is `docs/dev/dsl/modifiers.md`.
- A raw colour is not expressible in app code (theme authoring is where values belong; only
  `.bgGradient` artwork, which is data, takes hex).
- An unknown role is a **compile error** — `.fg(Label)` gets "did you mean `label`?" — and a
  name RFC-0116 retired is an error naming its replacement (`.fg(onSurface)` → `label`,
  `.bg(accentSoft)` → `.tint(accent)`).
- A role may come from data: `.bg($props.tint)` takes a role name as a string (a name that is
  no role paints nothing).
