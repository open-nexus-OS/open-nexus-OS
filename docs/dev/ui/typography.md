<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Typography

This document defines the **default typography contract** for Open Nexus OS UI.

See also: curated font set + explicit CJK fallback chains in `docs/dev/ui/font-library.md`.

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
- **Emphasis**: Medium (500)
- **Strong emphasis**: Semibold (600)
- Avoid “Light” as a default UI weight; reserve it for large headlines only.

## Determinism requirements

- **Font versions are pinned** (build artifacts must not change when the host OS updates fonts).
- **Font fallback order is explicit** and stable across devices.
- Text measurement and layout must be deterministic (see `docs/dev/ui/text.md`).
