---
title: TASK-0058 UI v3a: deterministic layout engine (flex/grid/stack) + text wrapping + host goldens
status: In Progress
owner: @ui
created: 2025-12-23
updated: 2026-05-16 (production-grade, 31 tests, windowd integrated, single source of truth, pretext philosophy, types concretized)
depends-on: [TASK-0057, TASK-0056]
follow-up-tasks: [TASK-0059]
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - **RFC seed (SSOT contract)**: docs/rfcs/RFC-0057-ui-v3a-layout-engine-pretext-contract.md
  - Design reference: https://github.com/chenglou/pretext (prepare/layout split philosophy)
  - UI v2b shaping baseline: tasks/TASK-0057-ui-v2b-text-shaping-svg-pipeline.md
  - UI v2a present/input baseline: tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md
  - UI layout pipeline contract: docs/dev/ui/foundations/layout/layout-pipeline.md
  - Text preparation contract: docs/dev/ui/foundations/layout/text.md
  - Wrapping spec: docs/dev/ui/foundations/layout/wrapping.md
  - DSL alignment (view expressions, modifiers): tasks/TASK-0075-dsl-v0_1a-syntax-ir-cli.md
  - DSL syntax conventions: docs/dev/dsl/syntax.md
  - Testing contract: docs/dev/ui/foundations/quality/goldens.md
  - Minimal DisplayServer v0: docs/architecture/display-output-service-chain.md
  - Proof surface reference: source/services/windowd/src/os_lite.rs (current hardcoded panel)
---

## Context

UI v3 introduces "real UI structure": deterministic layout, text wrapping, and stable measurement contracts.
This must be **deterministic** and **testable headlessly** (host-first), so later windowd/plugins can
use it without layout drift.

This task is **v3a** (layout + wrapping). Clipping/scroll/effects/IME are deferred to v3b (`TASK-0059`).
It should still feed the shared visible proof surface: once a consumer mounts it, wrapped text and hit regions must be
visible on-screen, not only in host JSON goldens.

The design follows the **pretext philosophy** ([chenglou/pretext](https://github.com/chenglou/pretext)):
- **prepare** paragraph/run data independently of container width (shaping, bidi, segment boundaries),
- **layout** line breaks and positions for a given width bucket,
- **cache** each level separately so scroll and resize avoid redundant work.

The shaping stack is **pure Rust**: `rustybuzz` (HarfBuzz port) + `fontdue` for rasterization.
No C libraries, no system HarfBuzz.

### Current proof surface state

Minimal DisplayServer v0 (TASK-0057) currently uses hand-positioned proof-text overlay in `windowd/src/os_lite.rs`:
```rust
const PROOF_PANEL_X: u32 = 56;
const PROOF_PANEL_Y: u32 = 440;
const TARGET_ROW_Y: u32 = 600;
// Cards: PROOF_PANEL_X + 24 + n * (TARGET_CARD_W + TARGET_GAP)
```

Layout boxes from this task must replace that overlay with properly measured, wrapped, and positioned text blocks
plus card rows. The proof panel must render **pixel-identical** to the current hardcoded version as a regression gate,
then diverge intentionally as new layout features are added.

### DSL alignment (naming contract)

The DSL v0.1a (`TASK-0075`) defines these view expressions and modifiers relevant to layout:

- **View expressions**: `Stack`, `Text`, `Spacer`, `List` (Grid deferred)
- **Modifiers**: `padding(...)`, `margin(...)` — spacing is modifier-driven, not node-type-driven

The DSL uses `Stack` (not `VStack`/`HStack`). Direction is a property: `Stack(direction: column)` vs `Stack(direction: row)`.
The layout engine mirrors this: `Stack { direction: Direction::Column }` — not separate VStack/HStack types.
Additional properties (`gap`, `align`, `justify`) use Tailwind-inspired naming where the DSL does not yet define equivalents.

## Goal

Deliver:

1. `userspace/ui/layout` crate:
   - **`Stack`** (flex row/column) with `Direction::Column | Row`, `gap`, `padding`, `Align`, `Justify`
   - **`Grid`** v1 (fraction columns: `1fr 2fr 1fr`)
   - **`Spacer`** (flexible space filler with `flex_grow`)
   - **`FlexItem`** (child properties: `flex_grow`, `flex_shrink`, `align_self`, `margin`)
   - deterministic numeric rules (fixed-point or integer-only; no `f32`/`f64` in layout math)
   - stable hit/input bounds derived from layout boxes for `windowd`/SystemUI consumers
2. `MeasureText` callback trait:
   - decouples layout crate from `nexus-shape` (no direct dep on shaping crates)
   - `prepare(text, style) → PreparedTextHandle` — shaping, bidi (backs paragraph/run cache)
   - `measure_width(handle) → FxPx` — natural advance width
   - `layout_lines(handle, width, max_lines) → LineLayout` — width-dependent line breaking (backs line-layout cache)
3. Paragraph/run cache + line-layout cache split (`nexus-shape` extension):
   - explicit cache split as documented in `docs/dev/ui/foundations/layout/layout-pipeline.md`
   - `ParagraphKey = (text_hash, text_style, locale, bidi_mode, fallback_lane, whitespace_mode)`
   - `LineLayoutKey = (paragraph_key, width_bucket, wrapping_policy, max_lines)`
   - scroll and resize changes must not force text reshaping
4. Wrapping helpers in `userspace/ui/shape/src/wrap.rs`:
   - Unicode line breaking (minimal UAX#14 subset — excluding SHY, CM, SA classes)
   - ellipsis and max-lines truncation
   - test samples with excluded boundaries verifying fallback behavior
5. Host tests (`tests/ui_v3a_host/`):
   - layout JSON goldens (style trees → stable box outputs)
   - wrapping JSON goldens (multilingual line break points + advance sums)
   - PNG goldens (rendered layout tree with text: colored boxes + wrapped text → pixel buffer)
   - hit/input region outputs matching intended interactive bounds
   - flex grow/shrink edge cases, grid fraction sizing and gaps
   - place-only updates do not require full remeasure for unchanged subtrees
6. **windowd proof panel replacement**:
   - replace `draw_proof_surface_row()` hand-rolled math with layout-engine-driven positioning
   - same visual output (pixel-identical regression gate)
   - markers: `layout: engine on`, `text: wrapping on`
7. Visible proof-surface handoff:
   - define a deterministic wrapped-text target/card for the shared proof surface,
   - expose the same layout boxes and input bounds used by live hit-testing.

## Non-Goals

- Kernel changes.
- Scroll, clipping, effects, IME/text input (v3b).
- Full CSS or complete grid auto-placement.
- Soft hyphen insertion, dictionary-based hyphenation (SHY, CM, SA line-breaking classes deferred).
- The DSL itself (syntax, lowering, interpreter — those are DSL tasks).
- GPU acceleration.
- C libraries — only `rustybuzz` + `fontdue` (pure Rust).

## Constraints / invariants (hard requirements)

- Deterministic output:
  - no floating-point drift; use fixed-point or integer rounding rules that are documented.
  - stable traversal order (no hash-map iteration for layout children).
- Bounded compute:
  - `MAX_LAYOUT_NODES` cap per layout call,
  - `MAX_LAYOUT_DEPTH` cap for recursion.
- Invalidation/caching posture:
  - scroll and pure offset changes should not force text reshaping,
  - measurement and placement must be testable as separate concerns,
  - cache keys and rounding rules must be documented deterministically.
- Interaction posture:
  - layout outputs used by launcher, scroll containers, and controls must expose deterministic input/hit regions,
  - visual boxes and input boxes must not drift silently.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No C dependencies in the OS graph.

## Type system (normative reference: RFC-0057)

See `docs/rfcs/RFC-0057-ui-v3a-layout-engine-pretext-contract.md` for full definitions. Summary:

```rust
// ═══ Layout containers (core deliverable) ═══
pub enum Direction { Column, Row }
pub enum Align { Start, Center, End, Stretch }
pub enum Justify { Start, Center, End, SpaceBetween, SpaceAround, SpaceEvenly }
pub enum Overflow { Visible, Hidden }
pub enum Position { Relative, Absolute }
pub type ZIndex = i16;

pub struct Stack { direction, gap, padding, align, justify, overflow, flex_wrap, min/max_width, min/max_height }
pub struct Grid { columns: Vec<Fraction>, gap, row_gap, padding, overflow, min/max_width, min/max_height }
pub struct Spacer { flex_grow: u32, min_size: Option<FxPx> }       // invisible, no VisualStyle
pub struct FlexItem { flex_grow, flex_shrink, align_self, margin, position, z_index, min/max_width }

// ═══ Visual (added alongside containers — paint-only invalidation) ═══
pub struct Rgba8 { r, g, b, a: u8 }                                 // theme-independent; resolved from tokens by consumer
pub struct Border { width: FxPx, color: Rgba8 }
pub struct EdgeBorder { top, right, bottom, left: Option<Border> }
pub struct CornerRadius { top_left, top_right, bottom_right, bottom_left: FxPx }
pub struct VisualStyle { background: Option<Rgba8>, border: EdgeBorder, corner_radius: CornerRadius, opacity: Option<FxPx> }

// ═══ Text styling (added alongside — paragraph/run + line-layout cache split) ═══
pub enum TextAlign { Left, Center, Right }
pub enum LineHeight { Relative(FxPx), Absolute(FxPx) }               // Relative e.g. 1.5 stored as FxPx(150)
pub enum FontWeight { Regular=400, Medium=500, Semibold=600, Bold=700 }
pub enum WhiteSpace { Normal, Pre, NoWrap }
pub struct TextStyle { font_size: FxPx, font_weight: FontWeight, line_height: LineHeight, text_align: TextAlign, color: Rgba8, white_space: WhiteSpace }

// ═══ Layout tree (containers + text, each with VisualStyle) ═══
pub enum LayoutNode {
    Stack(Stack, VisualStyle, Vec<LayoutNode>),
    Grid(Grid, VisualStyle, Vec<LayoutNode>),
    Spacer(Spacer),                       // no VisualStyle
    Text(TextNode, VisualStyle),
}
pub struct TextNode { content: TextContent, style: TextStyle, max_lines: Option<u32>, min/max_width: Option<FxPx> }

// ═══ Measurement callback (decoupled from nexus-shape) ═══
pub trait MeasureText {
    fn prepare(&self, content: &TextContent, style: &TextStyle) -> PreparedTextHandle;
    fn measure_width(&self, handle: &PreparedTextHandle) -> FxPx;
    fn layout_lines(&self, handle: &PreparedTextHandle, width: FxPx, max_lines: Option<u32>) -> LineLayout;
}
```

### Theme integration

Layout crate uses concrete `Rgba8` only. Theme tokens resolved in consumer layer:

``` text
nxtheme.toml → nexus_theme::resolve("surface") → Rgba8 → VisualStyle::background
nxtheme.toml → nexus_theme::resolve("fg")       → Rgba8 → TextStyle::color
nxtheme.toml → nexus_theme::resolve("border")    → Rgba8 → EdgeBorder::all(px(1), color)
```

## Security

- Threat model: oversized nodes, deep nesting, div-by-zero in flex, Unicode bombs, cache exhaustion
- Invariants: MAX_LAYOUT_NODES, MAX_LAYOUT_DEPTH, fixed-point arithmetic, no unwrap/expect
- DON'T DO: no layout on untrusted markup, no metric leakage, no layout-for-access-control

## Red flags / decision points

- **YELLOW (UAX#14 completeness) — NEUTRALIZED**:
  - v3a implements a minimal line-breaking subset sufficient for deterministic wrapping.
  - Explicitly NOT implemented in v3a: SHY (soft hyphen), CM (combining mark), SA (complex scripts)
    line-breaking classes, dictionary-based hyphenation.
  - Tests must contain samples with these boundaries and verify fallback behavior (grapheme cluster
    boundaries or hard break at nearest allowed opportunity).

- **YELLOW (Fixed-point format) — OPEN**:
  - Exact format (16.16 vs 22.10 vs integer-only) to be decided during Phase 0 implementation.
  - Since rustybuzz naturally produces integer glyph advances, integer-only may suffice for v3a.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v3a_host/`:

- layout:
  - style trees → stable box outputs (JSON goldens)
  - style trees → stable input/hit region outputs matching intended interactive bounds
  - flex grow/shrink edge cases
  - grid fraction sizing and gaps
  - place-only updates do not require full remeasure for unchanged subtrees
- wrapping:
  - multilingual samples produce stable line break points and advance sums (JSON goldens)
  - ellipsis and max-lines truncation rules verified
  - paragraph prep reused across multiple width-bucket measurements
- PNG goldens:
  - layout tree → rendered colored boxes + wrapped text → stable pixel output
  - proof panel layout matches current hardcoded output (regression gate)

### Proof (OS/QEMU) — gated

Once windowd/plugins consume layout:

- `layout: engine on`
- `text: wrapping on`
- `SELFTEST: ui v3 wrap ok` (added in v3b integration task)
- Proof panel visually identical to pre-layout state (regression gate)

### Visual proof handoff — required

- the shared proof surface has a bounded text box/card whose wrapping is visibly stable,
- the visible text target uses the same layout bounds that later hover/focus/click proof paths consume,
- the proof panel is fully driven by layout engine output (no hardcoded positions remain).

## Touched paths (allowlist)

- `userspace/ui/layout/` (new)
- `userspace/ui/shape/src/wrap.rs` (new)
- `userspace/ui/shape/src/cache.rs` (extend: paragraph/run + line-layout caches)
- `source/services/windowd/src/os_lite.rs` (replace hardcoded proof panel with layout-driven positioning)
- `tests/ui_v3a_host/` (new)
- `docs/rfcs/RFC-0057-ui-v3a-layout-engine-pretext-contract.md` (contract seed)
- `docs/dev/ui/foundations/layout/layout.md` + `docs/dev/ui/foundations/layout/wrapping.md` + `docs/dev/ui/foundations/layout/layout-pipeline.md`

## Plan (small PRs)

1. **Layout crate**
   - `FxPx` fixed-point type, `Rect`, `EdgeInsets`
   - `Stack`, `Grid`, `Spacer`, `FlexItem` types
   - `MeasureText` trait
   - flex/stack/grid v1 algorithms with deterministic numeric handling

2. **Wrapping**
   - `userspace/ui/shape/src/wrap.rs`: minimal UAX#14 line break opportunities + truncation/ellipsis
   - `userspace/ui/shape/src/cache.rs`: paragraph/run cache + line-layout cache
   - `MeasureText` implementation backed by `nexus-shape`

3. **Tests**
   - `tests/ui_v3a_host/`: JSON goldens for layout and wrapping
   - PNG goldens for rendered layout trees (colored boxes + text)
   - Negative tests: node count overflow, depth overflow, div-by-zero

4. **windowd integration**
   - Replace `draw_proof_surface_row()` with layout-tree-driven rendering
   - Pixel-identical regression gate
   - Markers: `layout: engine on`, `text: wrapping on`

5. **Docs**
   - Determinism rules and unsupported features
   - Update `layout-pipeline.md` with pretext reference
