---
schema: 1
id: RFC-0116
title: "One token set: the design artifacts' roles, text styles, grid, shapes and materials"
status: in_progress
owners: ["@ui"]
created: 2026-11-05
updated: 2026-11-09
subsystem: ui
tasks: [TASK-0353]
depends_on: [RFC-0070]
links:
  - label: "Related RFCs"
    target: "`docs/rfcs/RFC-0070-ui-design-system-ssot-convergence.md` (the design artifacts are the SSOT)"
  - label: "Related RFCs"
    target: "`docs/rfcs/RFC-0082-type-ramp-v2-on-glass-surface-tokens.md` (its type ramp and on-glass roles are replaced here)"
  - label: "Related RFCs"
    target: "`docs/rfcs/RFC-0109-surface-layers-the-app-scrolls-and-hovers-the-compositor-composes.md` (the layers the hover fill and the materials land on)"
  - label: "Design contract"
    target: "the design artifacts (design system: tokens, rules, components)"
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# RFC-0116: One token set — the design artifacts' roles, text styles, grid, shapes and materials

## Status at a Glance

- **Phase 0 (the contract)**: ✅ — this document; the DSL surface and the compositor's buckets
  written down with Phase 2.
- **Phase 1 (the themes and the typed tokens carry the set)**: ✅ 2026-11-09, TASK-0353 P2
  (36 roles since the 2026-11-10 amendment, 13 text styles, the scales and the four backdrops in light, dark and high
  contrast; the old keys gone; `every_role_resolves_in_every_theme`, the typed tokens' tests).
- **Phase 2 (the DSL names only the set)**: ✅ 2026-11-09, TASK-0353 P2 (all `.nx` sources
  migrated, goldens re-blessed; `userspace/dsl/core/tests/token_set.rs`,
  `token_vocabulary_lockstep`).
- **Phase 3 (the primitives follow the rules)**: ⬜ TASK-0353 P7.

## Why

The repository speaks a vocabulary of its own that grew beside the design: about 130 theme keys
(`glassTextPrimary`, `sidebarFg`, `mutedFg`, `surfaceAlt`, `accentSoft` …), 47 colour roles a
`.fg`/`.bg` may name, a size ramp (`textSize(xs … hero)`) with separate weight and leading, and
eight materials. The design artifacts — the design contract since the RFC-0070 amendment —
reduced theirs to one set: 31 colour roles, 13 text styles, a 4-pt grid, five radii, six shadows,
four materials and five control heights, plus rules for which surface blurs, which shape a control
takes and how hover shows. Two vocabularies drift (every screen of the artifacts needs a
translation table), and an app author cannot tell which of three names for a hairline is right.
One set, the artifacts', and nothing else.

## Scope

- **Owns:** the token vocabulary (colour roles, text styles, the spacing grid, radii, shadows,
  materials, control heights); how the themes carry it; the names the DSL accepts; the rules the
  primitives follow (shape by role, one blur per stack, the hover fill layer, concentric radii).
- **Does not own:** motion tokens (RFC-0109 amendment, TASK-0354); the compositor's composition of
  layers (RFC-0109, RFC-0101); the glyph atlas mechanics (RFC-0082 keeps the atlas, this RFC sets
  the sizes it must hold); the design itself (the artifacts).

## Contract (normative)

### Colour roles

Exactly these, with a value per theme (light, dark; high contrast maps onto the same roles):

| DSL name | role |
|---|---|
| `background` | opaque page area behind content that is not on the wallpaper |
| `label`, `labelSecondary`, `labelTertiary` | text and icons: content · metadata and section labels · placeholders and disabled only |
| `separator` | every hairline (dividers, list separators, pane and chip borders) |
| `fill` | resting fill of a control on a surface (switch-off track, segment, slider track, field, chip, secondary button) |
| `fillHover`, `fillActive` | the hover and press layer — shown by opacity, never set as a background |
| `glassPanel`, `glassWindow` | the two blur surfaces: panels, dock and launcher sheets · app windows |
| `glassCard` | a translucent layer on a blur surface (cards, grouped rows, tiles); never blurs |
| `glassOverlay` | a floating surface's frosted glass (modal, alert, menu, popover, toast, select list, action bar), four fifths opaque over the regular blur (amendment 2026-11-10) |
| `glassEdge` | the 1 px light edge of every glass layer |
| `surfacePane` | the opaque pane inside a window (occludes the window's blur) |
| `surfaceOverlay` | the opaque floating surface: the on-screen keyboard's tray, and what a floating surface must not show through |
| `scrim` | the dimming layer behind a modal overlay |
| `primary`, `onPrimary` | the brand's solid button and its label |
| `accent`, `accentGlow`, `onAccent` | the one tint (on-states, selection, links, focus) · its glow · text on it and on danger |
| `danger`, `success`, `warning` | status |
| `modeAirplane`, `modeDesktop`, `modeTablet` | the mode toggles' on-states |
| `island` | the activity runner's background |
| `chart1` … `chart5` | chart series |
| `wallpaperTint` | the wash over the wallpaper that keeps content legible (the artifacts' readme: black 35 % dark, white 20 % light) |
| `labelShadow` | the halo behind an icon label on the wallpaper (the artifacts' component stylesheet: a light glow in light, a dark one in dark) |
| `transparent` | no colour |

The last two are design values the artifacts keep outside their token file (the readme and the
component stylesheet); they are roles here so the shell names them like every other colour. A
tinted status background is the role at a fixed alpha computed by the primitive (`tintWash`),
never a role of its own; the hover and press layer on a coloured fill (white 14 %, black 12 %) is a
constant of that layer, not a role.

### Text styles

`.textStyle(name)` sets size, line height, weight and tracking together; `.fontWeight(light |
normal | medium | semibold | bold)` is the only override. No other size exists.

| DSL name | size / line (px) | weight | tracking |
|---|---|---|---|
| `clock` | 128 / 128 | 300 | −0.02 em, tabular digits |
| `display` | 64 / 64 | 300 | −0.02 em |
| `largeTitle` | 30 / 36 | 700 | −0.02 em |
| `title1` | 24 / 30 | 700 | −0.015 em |
| `title2` | 20 / 26 | 600 | −0.01 em |
| `title3` | 17 / 22 | 600 | −0.005 em |
| `headline` | 15 / 20 | 600 | 0 |
| `body` | 14 / 20 | 400 | 0 |
| `subheadline` | 13 / 18 | 400 | 0 |
| `footnote` | 12 / 16 | 400 | 0 |
| `caption` | 11 / 14 | 500 | +0.01 em |
| `caption2` | 10 / 13 | 500 | +0.02 em |
| `overline` | 11 / 14 | 600 | +0.06 em, upper case |

`clock` is the lock screen's time, the artifacts' display × 2 (operator decision 2026-11-09): their
fluid `clamp(64, 12vw, 128)` is two styles, `clock` on the regular and wide size classes and
`display` on compact. Both are digits-only faces whose digits have one advance (tabular).

**Figures** (amendment 2026-11-09, the artifacts' `tabular-nums`): a style's digits are
`proportional` or `tabular` (the theme key `figures`, required on every style; `clock` is
tabular). `.figures(tabular | proportional)` overrides it per text — counts, clocks, values that
change in place. Tabular digits advance by the face's widest digit, centred in that cell, and are
never kerned; the measure and the painter place them through the same pen.

### Spacing, radii, shadows, materials, control heights

- **Spacing** is a 4-pt grid in px: 0, 2, 4, 6 (only between an icon and its label), 8, 12, 16,
  20, 24, 32, 40, 48 — semantic names `gapInline` 8, `gapGroup` 12, `gapSection` 24,
  `marginLayout` 16. An off-grid value is a compile error in an app.
- **Radii**: `sm` 8, `md` 12, `lg` 16, `xl` 24, `full`. The outermost surface is `xl`; a
  container inside is concentric (parent radius − padding, never below `sm`).
- **Shadows**: `shadow1` (knobs), `shadow2` (app icons, chips), `shadow3` (menus, popovers,
  toasts, dock), `shadow4` (panels, modals, windows), `shadowInset` (wells and tracks),
  `edgeHighlight` (the 1 px top inset of a raised surface).
- **Materials**: only a root surface blurs — on the wallpaper `panel` and `window` (regular:
  40 px, saturation 180 %), `dock` (thick: 64 px, 180 %), `thin` and `ultraThin` (20 px, 8 px)
  for small standalone pieces; `overlay`, everything that floats, is `glassOverlay` over the
  regular blur of what it floats on, and opaque where it is nested in another surface (it
  cannot blur there); `card` is a translucent layer without blur; `pane` is opaque. A blur
  inside a blur does not compile.

  **Amendment 2026-11-10 (frosted overlays).** The artifacts' v3 `Menu` draws an opaque
  `surface-overlay`; the operator asked for the blur back on the board ("der Blur vom
  Dropdown-Menü ist zu wenig"). The design system's own glass overlay (`--glass-overlay-bg`,
  `rgba(252,252,254,.82)` light, `rgba(34,39,52,.80)` dark, the large blur) is the recipe:
  the new role `glassOverlay`, the `overlay` material on the regular backdrop as a root, opaque
  nested. Reduce transparency maps `glassOverlay` to the lighter solid grey, unblurred.
- **Control heights**: `xs` 24, `sm` 30, `md` 36 (default), `lg` 44, `xl` 52, each with its
  label style (`caption`, `footnote`, `body` medium, `headline`, `title3`).

### Rules the primitives follow

- **The shape follows the role:** an action control is a capsule, an icon-only control a circle,
  a value field a rounded rectangle by size (`sm` for xs/sm, `md` for md/lg, `lg` for xl), a
  container takes the computed concentric radius. A side touching the screen edge is square.
- **Hover never repaints:** hover and press show `fillHover`/`fillActive` on a layer between the
  background and the label, by opacity alone; only opacity and transform animate.
- **Borders:** glass layers `glassEdge` 1 px; hairlines and pane borders `separator`.

### DSL surface

What an app writes (the checker: `userspace/dsl/core/src/registry.rs`, `registry/vocab.rs`,
`check/modifiers.rs`, `check/materials.rs`; the runtime: `userspace/dsl/runtime/src/registry/`):

- **Colour** — `.fg`, `.bg`, `.borderColor`, `.bgFade` name a role. A role may come from data: a
  string literal naming a role is checked at compile time, an expression (`.bg($props.tint)`) is
  resolved at emit as a paint dependency, and a string that names no role paints nothing.
  `.tint(role[, fill | border | line])` (id 59) is the role as a wash background at 16 % (the
  default), 30 % or 55 % — the only alphas a role takes (`tintWash` above);
  `.borderTint(role)` (id 60) is a border in the role at 30 %.
- **Text** — `.textStyle(<style>)` (id 58) and `.fontWeight(light | normal | medium | semibold |
  bold)`; a text node without a style is `body`, `overline` sets its text in upper case.
  `.figures(tabular | proportional)` (id 62) sets the digits' width.
- **Spacing** — `.padding*`, `.gap`, `.margin`, `.rowGap` and `.hitSlop` take px on the grid or
  `gapInline | gapGroup | gapSection | marginLayout`; an off-grid integer or a fraction does not
  compile. The `Grid` widget's `rowGap:` is px too.
- **Shape and elevation** — `.rounded(sm | md | lg | xl | full | <px>)`; `.corners(tl, tr, br,
  bl)` (id 61), radii or px; `.shadow(a[, b])` with `shadow1`…`shadow4`, `shadowInset`,
  `edgeHighlight` and `halo` (the `labelShadow` behind a label on the wallpaper);
  `.border(hairline | ring)` (1 px, 2 px).
- **Materials** — `.material(panel | window | dock | thin | ultraThin | card | pane |
  overlay)`. The `Panel` widget is a panel with radius `xl` and padding `marginLayout`. A blur
  material inside a blur material — or inside a `Panel` — is NX0414.
- **Retired** — `textSize` (id 32), `leading` (35) and `textShadow` (50) keep their rows (ids are
  positional) and any use is a compile error naming the replacement; every retired token name
  errors with its replacement (`RETIRED_TOKENS`, `RETIRED_MODIFIERS`).

### Compositor regions

app-host declares a blur surface as a glass region only where it is the root of its stack (a
nested blur declares nothing and blends into its parent); the region's level is the bucket of
its backdrop (`source/libs/nexus-display-proto/src/client_surface.rs`):

| Material | Level | Backdrop |
|---|---|---|
| `panel` | `GLASS_PANEL` (0) | regular |
| `thin` | `GLASS_THIN` (1, the former card bucket) | thin, 20 px |
| `ultraThin` | `GLASS_ULTRA_THIN` (2, the former subtle bucket) | ultraThin, 8 px |
| `window` | `GLASS_WINDOW` (3) | regular |
| `dock` | `GLASS_DOCK` (5) | thick, 64 px |
| `overlay` (a root) | `GLASS_PANEL` (0) | regular |

Id 4 (`GLASS_OVERLAY`) is retired and never reused: a floating overlay takes the regular bucket.
`pane` — and a nested `overlay` — drawn at full opacity are opaque occluder regions; a `card` is
painted and declares no region. windowd reads each
bucket's blur radius from the theme's backdrops, and its own colours from the typed tokens. A
floating app window's page base is `glassWindow`, a fullscreen or overlay surface's
`background`; app-host's hover patch is `fillHover`.

### Versioning

The set is versioned with the design artifacts' token file (`version` there); a change to a name is
a change to this RFC. No alias of a removed name is kept.

## Invariants

- The DSL accepts only these names: `test_reject_an_old_token_name` (e.g. `.fg(onSurface)`),
  `test_reject_an_off_grid_spacing`, `test_reject_a_text_size_outside_the_styles`.
- Every role resolves in light, dark and high contrast; a theme missing one fails the build of the
  typed tokens (`userspace/ui/theme-tokens/build.rs`; on the host
  `every_role_resolves_in_every_theme`, `dark_and_high_contrast_author_every_theme_varying_role`).
- One blur per stack: `test_reject_a_material_inside_a_material`.

## Failure model

An unknown or removed name is a compile error with the replacement named in the message. A theme
missing a role fails the build, never a silent default at runtime.

## Migration

Every use of an old name is rewritten to the set in the same change (themes, typed tokens, DSL
registries, primitives, apps); the map from old to new is the design artifacts' token map plus the
repository's own names (TASK-0353 P2). RFC-0082's type ramp (`textSize`, `leading`) and its
on-glass roles (`onGlass*`) are replaced by the text styles and the label roles; its atlas stays.

## Proof

- Host: the DSL rejects every old name; the theme tests resolve every role in every theme; the
  primitives' goldens in light and dark.
- Repository: a search finds no old name in themes, code, apps or living docs.
- Board: the screens of the artifacts acked in light and dark (TASK-0353 P3).
