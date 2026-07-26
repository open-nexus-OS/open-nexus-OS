<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Typography

This document defines the **default typography contract** for Open Nexus OS UI.

See also: curated font set + explicit CJK fallback chains in `docs/dev/ui/foundations/visual/font-library.md`.

## Typeface choices (default)

- **UI Sans**: **Inter** (SIL OFL 1.1)
  - Rationale: modern, very readable at small sizes, good hinting, works well as a “system-like” UI font.
- **Fallback Sans**: **Noto Sans** (SIL OFL 1.1)
  - Rationale: broad Unicode coverage for international text; used when Inter lacks glyphs.
- **UI Mono (optional)**: pick one mono family and treat it as part of the design system contract (e.g. for DevTools/terminal surfaces).

Notes:
- macOS **SF Pro** is a great reference for “feel”, but it is **not open source** and should not be treated as a bundlable default.

## Weight guidance

- **Default body/UI**: Regular (400)
- **Emphasis**: Medium (500) — collapses onto Regular in the baked ladder
- **Strong emphasis**: Semibold (600)
- Avoid “Light” as a default UI weight; reserve it for large headlines only.
  The ladder enforces this: Light is baked at 120px and nowhere else.

## What the platform actually rasterizes (RFC-0082)

Text is drawn from **build-time baked A8 glyph atlases** — there is no runtime
font parsing anywhere. The bake is a sparse `(size, weight)` ladder, and a
`TextStyle` resolves to it through `FontSize::nearest(px, weight)`, whose rule
is **size wins over weight**:

| px | weights | charset |
|---|---|---|
| 13 | Regular, SemiBold | full (Latin + CJK) / Latin |
| 16 | Regular, SemiBold | full (Latin + CJK) / Latin |
| 21 | Regular, SemiBold | Latin |
| 36 | SemiBold | Latin |
| 120 | Light | digits only (`0-9 : . -`) |

Consequences worth knowing before you design against it:

- A **full** charset above 16px is forbidden — the hangul syllable block alone
  would add hundreds of megabytes to an atlas that is mapped into every
  app-host as one shared read-only VMO (RFC-0080).
- A CJK codepoint in a Latin/Digits face falls back to the **16px** full face.
  Mixed-script runs at 21px+ therefore render CJK smaller. Bounded and
  documented, not accidental.
- `hero` (120px) is for **numerals** — a clock. Letters at that size come from
  the fallback face.
- Adding a rung is a four-line change in `userspace/ui/text-baked/build.rs`
  plus a size assert.

Light 300 and SemiBold 600 do not exist as instances `fontdue` can reach, so
they are instanced on Inter's `wght` axis with `ttf-parser` and filled by the
build script's own nonzero-winding scanline rasterizer. Regular 400 keeps
going through `fontdue` so the 13/16px coverage stays byte-identical.

**Not available:** letter-spacing / tracking, OpenType feature control, and
italics. Tabular figures exist only as a property of the baked hero face
(unkerned, uniform numeral advance) so a ticking clock does not shuffle.

## Determinism requirements

- **Font versions are pinned** (build artifacts must not change when the host OS updates fonts).
- **Font fallback order is explicit** and stable across devices.
- Text measurement and layout must be deterministic (see `docs/dev/ui/foundations/layout/text.md`).
