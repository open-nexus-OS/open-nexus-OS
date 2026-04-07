<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Layout Pipeline

This document defines the default retained layout pipeline for the Open Nexus OS UI stack.
It is the contract that lets the DSL, interpreter/runtime, and native widgets stay deterministic while still scaling to
large lists, responsive shells, chat timelines, and grid-heavy surfaces.

The goal is:

- deterministic text and layout outputs,
- bounded and explainable invalidation,
- cheap scroll and resize reactions,
- and stable virtualization behavior for large collections.

See also:

- `docs/dev/ui/foundations/layout/layout.md`
- `docs/dev/ui/foundations/layout/text.md`
- `docs/dev/ui/collections/widgets/virtual-list.md`
- `docs/dev/ui/collections/lazy-loading.md`
- `docs/dev/dsl/ir.md`

## Non-goals

This contract is intentionally narrow.

It is not:

- a full CSS layout engine clone,
- a justification to force every surface through one generic widget abstraction,
- a generic parallel layout framework without explicit ownership/thread-affinity rules,
- or a license to trade deterministic behavior for speculative heuristics.

## When this contract applies

Use this pipeline for:

- normal DSL-authored UI trees,
- SystemUI surfaces,
- list/grid/detail app shells,
- text-heavy surfaces (settings, notifications, search, chat, feeds, files),
- and any native widget shell that still participates in host snapshots/goldens.

For data-heavy shells, pair this pipeline with:

- pure QuerySpec state for filter/order/page inputs,
- effect-side query execution,
- and the shared lazy-data contract when result sets are large enough to virtualize.

Do not force heavy interactive canvases into this pipeline when a bounded native surface is the better fit.
Maps canvases, timelines, waveforms, video previews, and similar workloads should use the DSL as the shell and a
`NativeWidget` or equivalent bounded surface for the specialized rendering core.

## Pipeline phases

The default frame/update pipeline is:

1. **Resolve tree**
   - Evaluate the declarative view tree against `$state`, `device.*`, theme, locale, and effect results.
   - Produce a deterministic retained tree with stable node identity.
2. **Text preparation**
   - Shape text into paragraph/run data using deterministic font fallback and bidi rules.
   - This phase must not depend on paint timing or wallclock state.
3. **Measure**
   - Compute intrinsic sizes and final measured sizes under constraints.
   - Text wrapping and line breaking happen here using prepared text data plus available width buckets.
4. **Placement**
   - Assign final rects/offsets to already measured nodes.
   - Scroll offset changes should normally affect placement, not text preparation.
5. **Paint eligibility / scene submission**
   - Decide which nodes produce visible scene primitives and which regions are dirty.
   - Submission remains damage-aware per `docs/dev/ui/foundations/quality/performance-philosophy.md`.

Phases must stay explicit in code and tests. A task may optimize or fuse internal steps, but it must preserve the same
deterministic external behavior.

## Ownership and threading model

The pipeline should follow a simple ownership model:

- the runtime owns the retained tree and invalidation state,
- the text subsystem owns prepared paragraph/run artifacts and line-layout artifacts,
- the layout subsystem owns measurement and placement caches,
- the renderer consumes immutable scene-submission inputs,
- and native widgets own only their specialized rendering/input core, not the shell's retained-tree authority.

Recommended thread posture:

- retained tree mutation, invalidation decisions, focus state, and viewport state are thread-affine by default,
- prepared text artifacts and other immutable derived outputs may be shared only when their construction and cache keys
  are deterministic,
- any background preparation must feed results back into the runtime through an explicit handoff, never via hidden
  shared mutable state.

Do not introduce implicit cross-thread mutation for “fast paths”.
If a subsystem is intentionally single-threaded, keep it explicitly thread-affine rather than accidentally `Send`/`Sync`.

## Stable identity

The retained tree must keep stable identity so work can be reused safely.

Required posture:

- list-like children use stable keys,
- reusable subtrees keep stable node identity across equivalent recomputations,
- text/layout caches must key off stable semantic inputs rather than transient object addresses,
- diff/rebuild order must not depend on filesystem order or hash-map iteration order.

Practical guidance:

- list rows/cards use domain ids (`notif.id`, `message.id`, `docId`, `appId`),
- template-like UI rows should keep a stable component/type identity plus a stable item key,
- profile/layout branches should preserve the same key space where the semantic node is “the same thing”.

## Invalidation matrix

The default mapping should be:

| Change | Default class | Notes |
| --- | --- | --- |
| theme color/token only | `paint-only` | no geometry or wrapping changes |
| opacity/z-order-only overlay update | `paint-only` | placement unchanged |
| scroll offset | `place-only` | anchor stays stable |
| sticky header progress / sheet offset | `place-only` | measurement unchanged |
| width bucket change | `measure+place` | text prep reused where shaping inputs are stable |
| expand/collapse row | `measure+place` | local subtree preferred |
| filter/reorder in a list | `measure+place` | placement may dominate; visible range stays keyed |
| text content change | `text-prep+measure+place` | new paragraph/run data required |
| locale change affecting line breaking | `text-prep+measure+place` | clear prepared text caches as needed |
| font fallback lane change | `text-prep+measure+place` | shaping inputs changed |
| thumbnail/image bytes arrive with fixed reserved box | `paint-only` | if geometry was already reserved |
| thumbnail/image bytes arrive and change intrinsic size | `measure+place` | reserve-box strategy preferred when possible |

## Invalidation classes

Every state/theme/locale/viewport change should map to one of these invalidation classes:

- **paint-only**
  - color/token/opacity/icon tint changed; geometry and wrapping unchanged
- **place-only**
  - scroll offset, alignment offset, overlay position, or other pure rect movement changed
- **measure+place**
  - constraints, available width bucket, visibility mode, expand/collapse, or item content size changed
- **text-prep+measure+place**
  - text content, locale, font fallback lane, bidi mode, or shaping-relevant typography changed

The runtime should prefer the cheapest valid class. If in doubt, choose correctness first, but document why a wider
invalidation was necessary.

## Rust posture

Rust's type system should help keep this contract honest.

Recommended posture:

- use newtypes for layout-facing ids and units instead of raw integers,
- keep derived artifacts immutable after creation,
- make thread-affinity explicit for runtime-owned mutable structures,
- and use `#[must_use]` on plan/result objects that must not be silently dropped.

Good candidate newtypes:

- `NodeId`
- `SubtreeHash`
- `ParagraphId`
- `WidthBucket`
- `ViewportPx`
- `LineCount`
- `RowHeightPx`
- `CacheBudgetBytes`

Good candidates for explicit `#[must_use]`:

- invalidation plans,
- measure/placement results,
- visible-range computations,
- anchor-correction results,
- cache insert/eviction reports used by correctness-sensitive paths.

`Send` / `Sync` posture:

- runtime state, retained tree mutation, focus state, and viewport mutation should default to thread-affine,
- immutable prepared text and immutable line-layout artifacts may be shared only with an explicit safety story,
- avoid “accidental” `Send`/`Sync` on caches that contain backend handles, font-engine state, or mutable scratch space.

## Text preparation contract

Text preparation exists to make text-heavy UI deterministic and reusable.

Recommended split:

- **paragraph/run cache**
  - keyed by content, text style, locale, bidi mode, and fallback chain
  - independent of container width
- **line layout cache**
  - keyed by paragraph cache entry plus width bucket and wrapping policy
  - stores line breaks, truncation, ellipsis decisions, and final advance sums

Do:

- use fixed-point or documented integer rounding,
- keep fallback order explicit and deterministic,
- make wrapping decisions fixture-testable,
- cap cache memory and eviction deterministically.

Do not:

- shape the same paragraph every frame because scroll moved,
- key caches by raw pointers,
- let host font discovery change results,
- or mix paint-only token changes into shaping cache keys.

Suggested cache keys:

- `ParagraphKey = { text_hash, text_style, locale, bidi_mode, fallback_lane, white_space_mode }`
- `LineLayoutKey = { paragraph_key, width_bucket, wrapping_policy, line_height_policy, max_lines }`

## Measurement contract

Measurement should answer one question: “Given these constraints, what size does this node need?”

Recommended posture:

- measurement is pure for a given input tuple,
- parent constraints flow down deterministically,
- intrinsic sizes are reusable when only placement changed,
- width-sensitive measurement should bucket widths when exact-pixel precision is not semantically required.

Width buckets are especially important for:

- responsive shells (`compact`, `regular`, `wide`),
- launcher/file/store grids,
- settings and notifications sidebars/details,
- chat/feed cards with mixed heights,
- and search/result overlays that should not fully re-measure on every pixel delta during resize.

Suggested measurement-facing keys:

- `RowHeightKey = { template_id, item_kind, width_bucket, density/profile lane }`
- `GridTrackKey = { container_kind, width_bucket, gap/rhythm tokens, density/profile lane }`

## Placement contract

Placement should not redo expensive measurement work unless it must.

Typical place-only updates:

- scrolling,
- sticky header offsets,
- viewport clipping offsets,
- anchored overlay positioning,
- and reordered visible-window placement inside a virtual list.

Placement must keep stable traversal order and stable rounding rules so repeated runs produce the same rect vectors.

Backend notes:

- DOM backends should preserve this contract by avoiding unnecessary DOM readbacks and reflows,
- Canvas backends benefit the most because the retained tree and placement plan are fully caller-owned,
- SVG backends should update only affected node geometry/text attributes rather than rebuild the whole subtree,
- NativeWidget backends should expose shell-facing geometry contracts, even when the heavy inner renderer is custom.

## Virtual list and mixed-height contract

Virtualized collection surfaces are the primary consumers of this pipeline.

Required behavior:

- stable visible-range computation,
- stable anchor by item key,
- deterministic recycling,
- bounded row/cell cache sizes,
- and no scroll-jump when new pages arrive or rows are remeasured.

Recommended strategy:

- keep an estimated size model for unseen rows,
- replace estimates with measured heights when rows become visible,
- anchor scroll position to a stable leading item key plus offset,
- remeasure only affected rows when content or width bucket changes,
- and prefer template-specific caches for common row/card shapes.

Placeholder posture:

- placeholders must have deterministic template ids and width-bucket-aware heights,
- placeholder replacement must preserve the anchor contract,
- and page arrival should invalidate only affected items/ranges instead of the full collection.

This is especially important for:

- notifications,
- chat transcripts,
- social/feed timelines,
- files list/grid,
- launcher/app grids,
- store/search result lists,
- and settings lists with expandable sections.

## Resize and responsive behavior

Resize reactions should be cheap and predictable.

Recommended rules:

- branch responsively using the deterministic `device.*` environment first,
- prefer shared shells plus width buckets over per-pixel bespoke layouts,
- do a real remeasure when crossing a semantic breakpoint or width bucket,
- avoid full-tree remeasure when only one container subtree changed,
- keep resize handling free of timing heuristics.

Good examples:

- launcher grid recomputes column count when bucket changes,
- settings sidebar/detail shell remeasures only the shell subtree,
- chat/feed rows reuse paragraph prep and only redo line layout for a new width bucket.

## Cache rules

All caches in this pipeline must have:

- explicit keys,
- explicit budgets,
- deterministic eviction,
- and tests for reuse and invalidation behavior.

Suggested cache families:

- text paragraph/run cache,
- line layout cache,
- intrinsic measurement cache,
- row/cell height cache,
- grid track sizing cache,
- placement cache for stable templates,
- thumbnail-independent geometry cache for file/media grids.

Eviction should prefer simple deterministic policies such as LRU with stable tie-breaks.
Never let cache presence change correctness; it may only change cost.

## Surface recipes

### Launcher grids

- Cache app-tile label prep separately from grid placement.
- Recompute column tracks per width bucket, not per scroll frame.
- Search/filter should mostly change visible items and placement, not re-shape every tile label.

### Settings and notification center

- Treat settings rows as a small set of templates with reusable measurement.
- Deep-link open/focus should target a stable row key and scroll anchor.
- Notification rows should keep mixed-height caches and prepend-safe scroll anchoring.

### Chat, feeds, and social timelines

- Persist immutable message/post snapshots so card measurement can be reused safely.
- Separate text prep from width-dependent line layout.
- Use anchor-by-key for append and prepend operations.

### Files, store, and search

- Share list/grid measurement rules where possible.
- Keep thumbnail arrival from triggering unrelated text remeasurement.
- Result chips and cards should have bounded template families and width-bucket caches.

### Maps and timeline-like apps

- Use the pipeline for side panels, sheets, route step lists, search results, bookmarks, and setting forms.
- Use bounded native surfaces for the map canvas, timeline canvas, waveform, or similar heavy rendering cores.

## Proof expectations

Host tests should prove:

- text prep keys are deterministic,
- measurement results are stable for a given width bucket,
- placement-only changes avoid unnecessary text/measure work,
- mixed-height virtualization keeps stable anchors,
- resize across buckets is deterministic,
- and cache eviction does not change layout correctness.

Helpful proof counters/metrics:

- count of text-prep runs,
- count of measure-only vs place-only passes,
- visible-range stability under repeated scroll sequences,
- anchor delta after prepend/resize,
- and cache hit/miss ratios for paragraph, line-layout, and row-height caches.

For end-to-end UI hot paths, relate these counters to broader system budgets:

- wakeups per interaction,
- queue transitions and queue residence time,
- recompute fanout per state mutation,
- observer count per commit,
- and useful vs wasted recomputes.

QEMU/OS proofs should only add markers when real consumers use the behavior.
Do not emit “fast path” markers without a real measured or observed behavior behind them.
