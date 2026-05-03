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

## Upstream candidate: BreezeX

Current candidate for baseline cursor theming:

- upstream: [ful1e5/BreezeX_Cursor](https://github.com/ful1e5/BreezeX_Cursor)
- package (Arch/AUR): [breezex-cursor-theme](https://aur.archlinux.org/packages/breezex-cursor-theme)
- store page: [BreezeX Black (KDE Store)](https://store.kde.org/p/1640747)

Notes:

- variants: `Dark`, `Light`, `Black`
- explicit HiDPI support and expanded cursor-size set
- license: GPL-3.0

## Mode Mapping Policy

Recommended default mapping:

- `light` mode -> `BreezeX-Black`
- `dark` mode -> `BreezeX-Light` (or `BreezeX-Dark` if visibility tests show better contrast)
- `black` mode (high-contrast) -> `BreezeX-Black` (required default)

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
theme_family = "breezex"

[cursor.mode]
light = "BreezeX-Black"
dark = "BreezeX-Light"
black = "BreezeX-Black"

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
