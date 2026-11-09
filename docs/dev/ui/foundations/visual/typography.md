<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Typography

This document defines the **default typography contract** for Open Nexus OS UI.
The text styles are the design artifacts' (RFC-0116); the atlas that draws them is
RFC-0082's (RFC-0080 shares it read-only with every app).

See also: curated font set + explicit CJK fallback chains in `docs/dev/ui/foundations/visual/font-library.md`.

## Typeface choices (default)

- **UI Sans**: **Inter** (SIL OFL 1.1)
  - Rationale: modern, very readable at small sizes, good hinting, works well as a “system-like” UI font.
- **Fallback Sans**: **Noto Sans** (SIL OFL 1.1)
  - Rationale: broad Unicode coverage for international text; used when Inter lacks glyphs.
- **UI Mono (optional)**: pick one mono family and treat it as part of the design system contract (e.g. for DevTools/terminal surfaces).

Notes:
- A proprietary system face may serve as a reference for “feel”, but it is **not open source** and is never a bundlable default.

## Text styles

The text styles set size, line height, weight and tracking together — `.textStyle(name)` in the
DSL, `TextStyleToken` in `nexus-theme-tokens`, `[textStyle.*]` in
`resources/themes/base.nxtheme.toml` (theme-invariant). `.fontWeight(light | normal | medium |
semibold | bold)` is the only override; no other size exists. A text without a style is `body`.

| Style | size / line (px) | weight | tracking |
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

`clock` and `display` are digits only; letters on them fall back (below). `clock` is the lock
screen's time, the artifacts' display × 2: their fluid `clamp(64, 12vw, 128)` is `clock` on the
regular and wide size classes and `display` on compact (the greeter); `display` is also the
calculator's number.

**Figures.** Every style declares `figures = "proportional" | "tabular"` in the theme; `clock` is
tabular. A text that changes in place — a count, a clock, a percentage, the calculator's number —
takes `.figures(tabular)`: each digit then advances by the face's widest digit, centred in that
cell, and is never kerned, so the number never shifts its neighbours (the artifacts'
`tabular-nums`; `nexus-text-baked`'s pen places it for the measure and the painter alike).

The control labels follow `ControlSize::label` (`userspace/ui/theme-tokens/src/design.rs`):
xs `caption`, sm `footnote`, md `body` medium, lg `headline`, xl `title3`.

## Weight guidance

- **Default body/UI**: Regular (400) — `body`, `subheadline`, `footnote`.
- **Emphasis**: Medium (500) — `caption`, `caption2`, the md control label (`body` medium).
- **Strong emphasis**: Semibold (600) — `headline`, `title3`, `title2`, `overline`.
- **Bold** (700) — `title1`, `largeTitle`.
- Light (300) exists only on the digits-only display faces, never for running text.

## What the platform rasterizes

Text is drawn from **build-time baked A8 glyph atlases** — there is no runtime font parsing
anywhere. The bake (`FACES` in `userspace/ui/text-baked/build.rs`) holds one face per style at
its default weight and one per weight override the apps use, named by size and weight
(`FontSize::S14W400`, …, `userspace/ui/text-baked/src/ladder.rs`).

| px | weights | charset |
|---|---|---|
| 10 | 500, 700 | Latin |
| 11 | 500, 600 | Latin |
| 12 | 400, 500, 600 | Latin |
| 13 | 400, 500, 600 | full (Latin + CJK) at 400, Latin otherwise |
| 14 | 400, 500, 600 | full at 400, Latin otherwise |
| 15, 17 | 600 | Latin |
| 20 | 400, 500, 600 | Latin |
| 24, 30 | 700 | Latin |
| 64, 128 | 300 | digits only (`0-9 : , . − - + × ÷ % =` and the space) — `display`, `clock` |

- **Resolution.** A style's size and weight always has its own face (`FontSize::exact`).
  Anything else — a `.textFit` size, an override no app uses yet — goes through
  `FontSize::nearest`: the largest baked size not above the request (text never draws larger
  than the box that asked for it), then the nearest weight baked at that size, a tie going to
  the heavier face. Size wins over weight: a size miss is far more visible than a weight miss.
- **The line box is the style's.** `body` is 20 px tall whatever the face's own ascent and
  descent; the painters centre the face's band in the line with half-leading
  (`BakedTextMeasure::line_box`). A `.textFit` size scales the line box with it.
- **Tracking** (letter-spacing, milli-em per style) is placed by the same pen in measurement,
  ellipsis and drawing, so a tracked run measures what it paints.
- **Fallback.** A codepoint a face lacks comes from the 14 px full face: CJK in a title renders
  small instead of blank, a letter on a digits-only face likewise.
- **Budget.** A full charset above 16 px fails the build — the hangul syllable block alone would
  add hundreds of megabytes to an atlas that is mapped into every app-host as one shared
  read-only VMO; the whole atlas — 22 faces, about 4.0 MB — stays under 5 MiB
  (`atlas_stays_within_the_budget`).
- Adding a face is one line in `FACES` plus its `FontSize` variant; a const check keeps the two
  one to one.

Regular 400 goes through `fontdue` (the variable font's default instance), so the 13 px full
face stays byte-identical across builds. The other weights (300, 500, 600, 700) are instanced on
Inter's `wght` axis with `ttf-parser` and filled by the build script's own scanline rasterizer
(`userspace/ui/text-baked/build/raster.rs`).

**Not available:** OpenType feature control and italics. Tabular figures exist only as a
property of the digits-only faces (`display`, `clock`: unkerned, one advance for every digit) so
a ticking clock does not shuffle.

## Determinism requirements

- **Font versions are pinned** (build artifacts must not change when the host OS updates fonts).
- **Font fallback order is explicit** and stable across devices.
- Text measurement and layout must be deterministic (see `docs/dev/ui/foundations/layout/text.md`).
