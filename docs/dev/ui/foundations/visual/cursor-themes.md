<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Cursor Themes

Cursor themes are part of visual foundations, not input-routing authority.
`windowd` keeps hit-test/focus/click authority; cursor theme policy only controls
visual pointer assets, variant mapping, and size selection.

## Goals

- deterministic cursor theme resolution across profiles and modes,
- high-visibility defaults for accessibility and high-contrast operation,
- HiDPI-safe sizing with bounded fallback behavior.

## Upstream candidate: Mocu

Current cursor theme (via git submodule):

- upstream: [sevmeyer/mocu-xcursor](https://github.com/sevmeyer/mocu-xcursor)
- variants: White, Black
- sizes: 24, 36, 48, 60, 72, 96
- HiDPI: ✅ (native multi-size SVG source)
- license: [CC0](https://creativecommons.org/publicdomain/zero/1.0) (Public Domain)

Notes:

- SVG sources with placeholder colours (`$0a0b0c/shadow`, `$1a1b1c/stroke`, `$fafbfc/fill`)
- Hotspot specified via `<circle id="hot">` in each SVG
- inspired by DMZ and cz-Viator cursor themes
- CC0 = Apache-2.0 compatible, no attribution required, commercial use unrestricted

## Mode Mapping Policy

Recommended default mapping:

- `light` mode -> `Mocu-Black`
- `dark` mode -> `Mocu-White`
- `black` mode (high-contrast) -> `Mocu-Black` (required default)

Rationale:

- preserve cursor edge contrast against both light and dark surfaces,
- keep one deterministic high-contrast default for accessibility mode.

## HiDPI Size Policy

Use deterministic logical tiers:

- `dpiClass=low` -> `24`
- `dpiClass=normal` -> `32`
- `dpiClass=high` -> `48`

If exact size is unavailable:

- choose nearest available size,
- prefer larger on ties,
- do not silently switch theme family.

## Runtime Resolution Contract

Resolution order (highest wins):

1. product/deployment override
2. profile override
3. global theme default

App-level overrides are disallowed by default and require explicit policy.

## Example Theme Snippet (Illustrative)

```toml
[cursor]
theme_family = "mocu"

[cursor.mode]
light = "Mocu-Black"
dark = "Mocu-White"
black = "Mocu-Black"

[cursor.size_by_dpi]
low = 24
normal = 32
high = 48

[cursor.fallback]
strategy = "nearest-available"
prefer_larger = true
```

## Accessibility Requirements

- `black` mode must always route through the high-contrast cursor mapping.
- Cursor visibility must stay legible over:
  - bright surfaces,
  - dark surfaces,
  - translucent/material overlays.
- HiDPI sizes must remain crisp at 2.0x+ display scaling.

## Validation Checklist

- verify all mode mappings resolve deterministically (`light`, `dark`, `black`),
- verify visibility on mixed-contrast UI surfaces,
- verify cursor crispness at low/normal/high dpi classes,
- verify fallback path with a missing exact size,
- verify no marker-only claims: visual checks must match routed pointer behavior evidence.
