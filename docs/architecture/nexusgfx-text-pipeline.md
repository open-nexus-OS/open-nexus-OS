# NexusGfx Text Pipeline Integration

**Created**: 2026-04-10  
**Owner**: @ui @runtime  
**Status**: Active architecture guidance; complements existing UI layout/text docs

---

## Purpose

This document explains how `NexusGfx` should **accelerate and submit text**
without redefining the canonical text/layout contracts that already exist in the
UI foundation docs.

This page is intentionally about:

- renderer-facing text caches,
- glyph/materialization strategy,
- batching and submission,
- and deterministic proof posture.

It is **not** a replacement for the canonical shaping/layout contracts.

---

## Canonical upstream docs

These existing documents remain the source of truth for text/layout behavior:

- `docs/dev/ui/foundations/layout/layout-pipeline.md`
- `docs/dev/ui/foundations/layout/layout.md`
- `docs/dev/ui/foundations/layout/text.md`
- `docs/dev/ui/collections/widgets/virtual-list.md`
- `docs/dev/ui/collections/lazy-loading.md`
- `docs/dev/dsl/ir.md`

`NexusGfx` must integrate with those contracts, not replace them.

---

## Architecture split

### Owned by the UI/layout/text stack

- resolve tree
- text preparation
- shaping and bidi
- width-bucket-sensitive line layout
- measure and placement
- invalidation classes
- stable identity and anchor-by-key behavior

### Owned by `NexusGfx`

- glyph/image materialization strategy
- atlas and cache lifetime
- batching
- damage-aware scene submission
- paint-time resource choice
- backend-specific text draw execution

This split is critical to avoid a second competing text architecture.

---

## Prepared paragraph integration

The UI stack already defines a width-independent prepared paragraph contract.

`NexusGfx` should consume those artifacts as stable inputs for paint-time
materialization and submission.

Rule:

- shaping/layout correctness is decided before rendering,
- renderer text caches key off the semantic text artifacts and style inputs,
- scroll-only changes should normally reuse prepared text and line layout.

---

## Glyph materialization posture

The initial rendering strategy should be conservative and practical.

### Preferred v1 posture

- rasterize glyphs or glyph fragments into atlas-like storage,
- cache by deterministic keys,
- reuse materialized glyph entries across frames,
- and make cache lifetime/budget bounded.

Suggested cache key components:

- font identity
- glyph identity
- font size / text style bucket
- subpixel-sensitive position bucket if needed
- color-emoji or multicolor mode where relevant

### Later optional posture

- runtime vector-based glyph rasterization
- higher-quality subpixel refinement
- mixed atlas/vector path

Those are future quality/perf improvements, not first-milestone requirements.

---

## Atlas posture

An atlas or atlas-like cache is the practical default for v1.

Expected properties:

- lazy fill on demand
- bounded eviction policy
- deterministic cache keys
- bounded atlas count/size
- no unbounded growth hidden behind convenience APIs

Why:

- works for UI and text-heavy apps,
- integrates well with retained layout identities,
- keeps host/CPU backend realistic.

---

## Damage-aware submission

Text rendering should integrate with the existing retained/damage-aware model.

Rule:

- text submission should be aligned with paint eligibility and dirty regions,
- theme/color-only changes should prefer `paint-only`,
- scroll changes should normally avoid reshaping or remeasuring,
- newly materialized glyphs should not trigger unrelated full-surface redraws.

This preserves the behavior promised by the existing layout pipeline docs.

---

## Batching posture

The renderer should batch text draw work where compatible, but batching must not
erase correctness boundaries.

Batch keys may include:

- atlas/resource identity
- blend mode / color mode
- clip/scissor compatibility
- transform compatibility
- pipeline/material state

Do not batch across incompatible state if it breaks deterministic output or
damage accounting.

---

## Virtual lists and lazy data

Large text-heavy lists already have a deterministic contract.

`NexusGfx` should preserve that by ensuring:

- anchor-by-key behavior is not broken by text cache churn,
- placeholder rows and measured rows can share rendering infrastructure,
- visible-range changes do not force full atlas/layout invalidation,
- and measurement/placement reuse remains more important than fancy text effects.

---

## Color glyphs and emoji

The architecture should plan for:

- monochrome glyphs,
- color glyphs / emoji,
- and fallback lanes.

But this should be modeled as a resource/materialization distinction, not a
different layout pipeline.

---

## Native widget boundary

Heavy specialized surfaces may use `NativeWidget` or another bounded native
surface as the rendering core, but:

- the shell still uses the retained layout authority,
- the shell still participates in goldens and damage rules,
- and text in the shell should still flow through the same canonical contracts.

---

## Deterministic proof posture

Text acceleration must be tested in the same spirit as the existing UI docs:

- deterministic host goldens
- bounded cache behavior
- stable invalidation classes
- explicit perf traces where useful

The first milestone should **not** promise a perfect final text renderer.
It should prove:

- compatibility with the layout/text contracts,
- bounded atlas/cache behavior,
- deterministic submission behavior.

---

## First milestone guidance

For the first extraction, prefer:

- atlas-based materialization
- deterministic batch keys
- cache budgeting
- damage-aware submission
- no new shaping/layout contract

This keeps `NexusGfx` aligned with the already-documented UI stack.

---

## Related

- `docs/dev/ui/foundations/layout/layout-pipeline.md`
- `docs/dev/ui/foundations/layout/text.md`
- `docs/dev/dsl/ir.md`
- `tasks/TRACK-NEXUSGFX-SDK.md`
