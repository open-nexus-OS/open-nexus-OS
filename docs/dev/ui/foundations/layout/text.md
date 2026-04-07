<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Text

This document covers text rendering fundamentals used by UI:

- shaping and bidi,
- line breaking,
- deterministic font fallback and measurement contracts.

Recommended posture:

- prepare paragraph/run data separately from width-dependent line layout,
- cache shaping results by text/style/locale/fallback inputs,
- and cache line layout by paragraph entry plus deterministic width bucket and wrapping policy.

## Prepared paragraph contract

A prepared paragraph is the width-independent text artifact used by the layout pipeline.

It should contain enough data to make later layout passes cheap and deterministic:

- normalized/canonicalized text according to the chosen whitespace mode,
- stable segment/grapheme boundaries,
- bidi/shaping results and fallback decisions,
- segment/run advances measured against the pinned font configuration,
- and cursors/ranges that later line layout can reuse without reparsing the original text.

It should not contain width-specific line breaks or placement.

Recommended split:

- prepare once per `{ text, style, locale, fallback lane, whitespace mode }`,
- layout many times per `{ prepared paragraph, width bucket, line-height / max-lines policy }`.

## Determinism posture

Text measurement stays deterministic only if the inputs are pinned.

The effective ground truth must include:

- pinned font versions,
- explicit fallback chain / fallback lane,
- locale,
- whitespace mode,
- line-breaking policy,
- and documented rounding behavior.

Theme/color changes should not invalidate prepared text unless they also change shaping-relevant typography.

See also:

- `docs/dev/ui/foundations/layout/layout-pipeline.md`
- `docs/dev/ui/foundations/rendering/text-stack.md`
