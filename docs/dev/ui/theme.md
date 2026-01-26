<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Theme & Tokens

Themes define token values (colors, typography scales, spacing, corner radii) used by components.

See also: `docs/dev/ui/colors.md` (semantic tokens + palettes + presets).
See also: `docs/dev/ui/materials-glass.md` (translucent “glass” materials + update policy).

Authoring:

- `*.nxtheme.toml` (human-editable)

Optional runtime artifact:

- `*.nxtheme` (Cap'n Proto; compiled)

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
