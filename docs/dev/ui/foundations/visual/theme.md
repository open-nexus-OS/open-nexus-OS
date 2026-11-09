<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Theme & Tokens

Themes define the values of the one token set (RFC-0116) that components use: the colour roles
per theme, and once for every theme the text styles, spacing names, radii, control heights,
shadows and backdrops.

See also: `docs/dev/ui/foundations/visual/colors.md` (the colour roles + palettes + presets).
See also: `docs/dev/ui/foundations/visual/typography.md` (the text styles and the atlas).
See also: `docs/dev/ui/foundations/visual/materials.md` (the materials, the backdrops + update policy).
See also: `docs/dev/ui/foundations/visual/cursor-themes.md` (cursor-family mapping, HiDPI sizes, high-contrast defaults).

Authoring:

- `resources/themes/*.nxtheme.toml` (human-editable). `base` carries the light values and every
  theme-invariant scale; `light` overrides nothing; `dark` and `highcontrast` override the
  roles and shadow colours that differ (high contrast also flattens the backdrops).

Optional runtime artifact:

- `*.nxtheme` (Cap'n Proto; compiled)

## Sections

The schema is closed: a section outside this list fails the parse
(`KNOWN_SECTIONS` in `userspace/ui/theme/src/schema.rs` — the retired `[typography]` and
`[leading]` among them).

| Section | Holds |
|---|---|
| `[tokens]` | the 35 colour roles (`colors.md`) |
| `[spacing]` | the semantic gaps: `gapInline` 8, `gapGroup` 12, `gapSection` 24, `marginLayout` 16 — the 4-pt grid itself is a closed list in code |
| `[radius]` | `sm` 8, `md` 12, `lg` 16, `xl` 24, `full` |
| `[control]` | the outer height of every control of a size: `xs` 24, `sm` 30, `md` 36, `lg` 44, `xl` 52 |
| `[textStyle.<name>]` | the text styles: `size`, `line`, `weight`, `trackingMilliEm`, `case` |
| `[shadow.<name>]` | `shadow1`…`shadow4`, `shadowInset`, `edgeHighlight`: `x`, `y`, `blur`, `spread`, `color`, `inset` |
| `[material.<backdrop>]` | `ultraThin`, `thin`, `regular`, `thick` (`type = "backdrop"`): `blurRadiusDp`, `saturationPct`, `downsampleFactor` |
| `[motion]` | durations in ms (the curves are the typed tokens' cubic-beziers) |
| `[zindex]` | stacking order (advisory) |
| `[icons]`, `[icons.symbols]` | the icon set and the symbol vocabulary (`icons.md`) |

`nexus-theme` (`userspace/ui/theme`) parses and validates the files; the build script of
`nexus-theme-tokens` (`userspace/ui/theme-tokens/build.rs`) generates the typed snapshots behind
the `Tokens` trait — `LightTokens`, `DarkTokens`, `HighContrastTokens`, wrapped by
`AccentTokens` (a user accent) and `SolidTokens` (reduce transparency). Its match arms are
exhaustive over the enums, and a theme-varying role a variant does not author fails the build,
so no role silently inherits a light value. The DSL, the kit widgets, app-host and windowd all
read these typed tokens.

## Cursor Theme Tokens

Cursor theming belongs to visual foundations and should be configured as deterministic
theme/profile policy (not ad-hoc app overrides).

Recommended minimum token surface:

- cursor theme family (`cursor.theme_family`)
- mode mapping (`light`, `dark`, `black`)
- size mapping by `dpiClass` (`low`, `normal`, `high`)
- fallback policy (`nearest-available`, tie-break rules)

For the cursor theme's defaults and accessibility constraints, use:

- `docs/dev/ui/foundations/visual/cursor-themes.md`

## Example (excerpt of `base.nxtheme.toml`)

```toml
[tokens]
label = "#000000e0"
separator = "#0000001a"
accent = "#2563eb"

[radius]
sm = 8
md = 12

[textStyle.body]
size = 14
line = 20
weight = 400
trackingMilliEm = 0
case = "none"
```
