<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Theme & Tokens

Themes define token values (colors, typography scales, spacing, corner radii) used by components.

See also: `docs/dev/ui/foundations/visual/colors.md` (semantic tokens + palettes + presets).
See also: `docs/dev/ui/foundations/visual/materials.md` (translucent “glass” materials + update policy).
See also: `docs/dev/ui/foundations/visual/cursor-themes.md` (cursor-family mapping, HiDPI sizes, high-contrast defaults).

Authoring:

- `*.nxtheme.toml` (human-editable)

Optional runtime artifact:

- `*.nxtheme` (Cap'n Proto; compiled)

## Cursor Theme Tokens

Cursor theming belongs to visual foundations and should be configured as deterministic
theme/profile policy (not ad-hoc app overrides).

Recommended minimum token surface:

- cursor theme family (`cursor.theme_family`)
- mode mapping (`light`, `dark`, `black`)
- size mapping by `dpiClass` (`low`, `normal`, `high`)
- fallback policy (`nearest-available`, tie-break rules)

For BreezeX-oriented defaults and accessibility constraints, use:

- `docs/dev/ui/foundations/visual/cursor-themes.md`

## Example (illustrative)

```toml
[colors]
accent = "#4F7DFF"
bg = "#0B0C10"
fg = "#EDEDED"

[radii]
sm = 6
md = 10
lg = 14
```
