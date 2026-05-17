# RFC-0057: UI v3a layout engine — deterministic flex/grid/stack + text wrapping contract seed

- Status: In Progress (production-grade: 31 tests, windowd integrated, no duplicate structure)
- Last Updated: 2026-05-15 (v2: +colors, border, TextStyle, VisualStyle, overflow, position, flex_wrap, z_index, white_space)
- Owners: @ui
- Created: 2026-05-15
- Links:
  - Tasks: `tasks/TASK-0058-ui-v3a-layout-wrapping-deterministic.md` (execution + proof)
  - Follow-up tasks: `tasks/TASK-0059-ui-v3b-clip-scroll-effects-ime-textinput.md`
  - Design reference: [chenglou/pretext](https://github.com/chenglou/pretext) — prepare/layout split philosophy
  - Related RFCs: `docs/rfcs/RFC-0056-*.md`, `docs/rfcs/RFC-0055-*.md`
  - DSL alignment: `docs/dev/dsl/syntax.md`, `tasks/TASK-0075-dsl-v0_1a-syntax-ir-cli.md`
  - Theme system: `docs/dev/ui/foundations/visual/theme.md`, `colors.md`, `typography.md`
  - Layout pipeline contract: `docs/dev/ui/foundations/layout/layout-pipeline.md`
  - Testing contract: `docs/dev/ui/foundations/quality/goldens.md`

## Status at a Glance

- **Phase 0 (Container layout)**: ⬜ — `Stack`/`Grid`/`Spacer` types + flex/grid algorithms + `FxPx`/`EdgeInsets`
- **Phase 1 (Visual + Text primitives)**: ⬜ — `Rgba8`, `Border`, `CornerRadius`, `VisualStyle`, `TextStyle`, `TextAlign`, `LineHeight`, `FontWeight`, `WhiteSpace`, `MeasureText` trait
- **Phase 2 (Text wrapping + caches)**: ⬜ — UAX#14 subset, ellipsis, paragraph/run + line-layout cache split
- **Phase 3 (Host tests)**: ⬜ — JSON goldens + PNG goldens
- **Phase 4 (windowd integration)**: ⬜ — proof panel replacement + `layout: engine on` marker

Definition: "Complete" means the contract is defined AND the proof gates are green.

## Scope boundaries (anti-drift)

- **This RFC owns**: deterministic layout algorithms (flex, grid, stack) with fixed-point arithmetic; visual primitives (`Rgba8`, `Border`, `CornerRadius`, `VisualStyle`); text styling (`TextAlign`, `LineHeight`, `FontWeight`, `TextStyle`, `WhiteSpace`); `MeasureText` callback, `Position` (relative/absolute), `ZIndex`, `flex_wrap`; text wrapping (UAX#14, ellipsis, max-lines), paragraph/run + line-layout cache split; golden test contract; windowd proof panel replacement
- **This RFC does NOT own**: scroll, clipping, effects, IME (TASK-0059); full CSS grid; soft hyphen/dictionary hyphenation; DSL syntax/lowering; kernel; GPU

## Goals

1. `userspace/ui/layout` crate with deterministic container layout (Stack/Grid/Spacer/flex_wrap)
2. Visual primitives (`Rgba8`, `Border`, `CornerRadius`, `VisualStyle`) — paint-only invalidation
3. Text primitives (`TextStyle`, `TextAlign`, `LineHeight`, `FontWeight`, `WhiteSpace`) + `MeasureText` trait
4. Text wrapping + paragraph/run + line-layout cache split (pretext model)
5. Host tests: JSON + PNG goldens; windowd proof panel replacement

## Non-Goals

Scroll, clipping, effects, IME (TASK-0059); full CSS grid; soft hyphen/dictionary hyphenation; DSL itself.

## Constraints / invariants

- **Deterministic**: no `f32`/`f64` in layout math; stable traversal order; integer-only `FxPx`
- **Bounded**: `MAX_LAYOUT_NODES`, `MAX_LAYOUT_DEPTH`, cache budgets with LRU eviction
- **Prepare/layout split**: paragraph cache (width-independent) + line-layout cache (width-dependent)
- **Paint-only invalidation**: `VisualStyle` separate struct from layout properties — type-level enforcement
- No `unwrap`/`expect`, no stubs claiming success

## Proposed design

### Rust type system (normative)

```rust
// Primitives
pub struct FxPx(pub i32);                    // Fixed-point pixel (integer-only for v3a)
pub struct Rect { pub x: FxPx, pub y: FxPx, pub width: FxPx, pub height: FxPx }
pub struct EdgeInsets { pub top: FxPx, pub right: FxPx, pub bottom: FxPx, pub left: FxPx }

// Layout direction and alignment
pub enum Direction { Column, Row }
pub enum Align { Start, Center, End, Stretch }          // cross-axis (items-*)
pub enum Justify { Start, Center, End, SpaceBetween, SpaceAround, SpaceEvenly } // main-axis
pub enum Overflow { Visible, Hidden }                    // v3b scissor clip boundary
pub enum Position { Relative, Absolute }                 // child positioning in Stack
pub type ZIndex = i16;                                   // stacking order (higher = on top)

// Container nodes
pub struct Stack {
    pub direction: Direction, pub gap: FxPx, pub padding: EdgeInsets,
    pub align: Align, pub justify: Justify, pub overflow: Overflow,
    pub flex_wrap: bool,                                 // wrap children to next row/column
    pub min_width: Option<FxPx>, pub max_width: Option<FxPx>,
    pub min_height: Option<FxPx>, pub max_height: Option<FxPx>,
}
pub struct Grid {
    pub columns: Vec<Fraction>, pub gap: FxPx, pub row_gap: Option<FxPx>,
    pub padding: EdgeInsets, pub overflow: Overflow,
    pub min_width: Option<FxPx>, pub max_width: Option<FxPx>,
    pub min_height: Option<FxPx>, pub max_height: Option<FxPx>,
}
pub struct Fraction(pub u32);

// Flex children
pub struct FlexItem {
    pub flex_grow: u32, pub flex_shrink: u32,
    pub align_self: Option<Align>, pub margin: EdgeInsets,
    pub position: Position, pub z_index: ZIndex,
    pub min_width: Option<FxPx>, pub max_width: Option<FxPx>,
}

// Visual (paint-only invalidation — separate struct from layout properties)
pub struct Rgba8 { pub r: u8, pub g: u8, pub b: u8, pub a: u8 }
pub struct Border { pub width: FxPx, pub color: Rgba8 }
pub struct EdgeBorder { pub top: Option<Border>, pub right: Option<Border>, pub bottom: Option<Border>, pub left: Option<Border> }
pub struct CornerRadius { pub top_left: FxPx, pub top_right: FxPx, pub bottom_right: FxPx, pub bottom_left: FxPx }
pub struct VisualStyle {
    pub background: Option<Rgba8>, pub border: EdgeBorder,
    pub corner_radius: CornerRadius, pub opacity: Option<FxPx>,
}

// Text styling
pub enum TextAlign { Left, Center, Right }
pub enum LineHeight { Relative(FxPx), Absolute(FxPx) }   // Relative e.g. 1.5 = FxPx(150)
pub enum FontWeight { Regular=400, Medium=500, Semibold=600, Bold=700 }
pub enum WhiteSpace { Normal, Pre, NoWrap }              // wrapping mode
pub struct TextStyle {
    pub font_size: FxPx, pub font_weight: FontWeight,
    pub line_height: LineHeight, pub text_align: TextAlign,
    pub color: Rgba8, pub white_space: WhiteSpace,
}

// Layout tree
pub enum LayoutNode {
    Stack(Stack, VisualStyle, Vec<LayoutNode>),
    Grid(Grid, VisualStyle, Vec<LayoutNode>),
    Spacer(Spacer),                          // invisible — no VisualStyle
    Text(TextNode, VisualStyle),
}
pub struct Spacer { pub flex_grow: u32, pub min_size: Option<FxPx> }
pub struct TextNode {
    pub content: TextContent, pub style: TextStyle,
    pub max_lines: Option<u32>, pub min_width: Option<FxPx>, pub max_width: Option<FxPx>,
}

// Measurement callback (decoupled from nexus-shape)
pub trait MeasureText {
    fn prepare(&self, content: &TextContent, style: &TextStyle) -> PreparedTextHandle;
    fn measure_width(&self, handle: &PreparedTextHandle) -> FxPx;
    fn layout_lines(&self, handle: &PreparedTextHandle, width: FxPx, max_lines: Option<u32>) -> LineLayout;
}
pub struct PreparedTextHandle(usize);
pub struct LineLayout { pub lines: Vec<LineMetrics>, pub natural_width: FxPx }
pub struct LineMetrics { pub text_range: core::ops::Range<usize>, pub width: FxPx, pub baseline: FxPx, pub height: FxPx }
```

### Theme token integration

Layout crate uses concrete `Rgba8` only. Resolution happens in consumer layer:

```
nxtheme.toml -> nexus_theme::resolve("surface") -> Rgba8 -> VisualStyle::background
nxtheme.toml -> nexus_theme::resolve("fg")       -> Rgba8 -> TextStyle::color
nxtheme.toml -> nexus_theme::resolve("border")    -> Rgba8 -> EdgeBorder::all(px(1), color)
```

### Naming rationale

| Concept | Rust type | DSL/Tailwind |
|---------|-----------|-------------|
| Container | `Stack { direction }` | `Stack(direction: column)` / `flex flex-col` |
| Grid | `Grid { columns }` | `grid-cols-3` |
| Spacer | `Spacer { flex_grow }` | `grow` |
| Gap | `gap`, `row_gap` | `gap-4`, `gap-y-4` |
| Padding | `EdgeInsets` (padding) | `p-4` |
| Margin | `EdgeInsets` (margin) | `m-4` |
| Alignment | `Align`, `Justify` | `items-center`, `justify-between` |
| Flex wrap | `flex_wrap: bool` | `flex-wrap` |
| Positioning | `Position::Absolute` | `absolute` |
| Z-order | `ZIndex` | `z-10` |
| Overflow | `Overflow::Hidden` | `overflow-hidden` |
| Background | `VisualStyle::background` | `bg(accent)` |
| Border | `EdgeBorder` | `border` |
| Radius | `CornerRadius` | `radius(md)` / `rounded-md` |
| Text align | `TextAlign` | `textAlign(center)` / `text-center` |
| Font weight | `FontWeight` | `fontWeight(semibold)` / `font-semibold` |
| Line height | `LineHeight` | `lineHeight(1.5)` / `leading-relaxed` |
| White space | `WhiteSpace` | `whiteSpace(nowrap)` / `whitespace-nowrap` |

### Text preparation contract (pretext model)

```
prepare(text, style) ---- once per content+style
  +-- shaping (rustybuzz) -> bidi -> segment boundaries -> glyph advances
  -> ParagraphKey = (text_hash, text_style, locale, bidi_mode, fallback_lane, whitespace_mode)
  -> PreparedParagraph (cached)

layout(paragraph, width, max_lines) -- per width bucket
  +-- UAX#14 line break -> width-constrained -> ellipsis / max-lines
  -> LineLayoutKey = (paragraph_key, width_bucket, wrapping_policy, max_lines)
  -> LineLayout (cached)
```

### Invalidation matrix (v3a -> v3b)

| Change | Class | v3a work | v3b work |
|--------|-------|----------|----------|
| theme color / VisualStyle | `paint-only` | none | repaint |
| scroll offset | `place-only` | none | reclip, reposition |
| width bucket change | `measure+place` | redo line layout | remeasure + reclip |
| text content change | `text-prep+measure+place` | reshape + relayout | full |

## Security considerations

- MAX_LAYOUT_NODES, MAX_LAYOUT_DEPTH, fixed-point div-by-zero prevention, Unicode bomb bounds, cache LRU eviction
- DON'T DO: layout on untrusted markup, unwrap/expect on measurement, cache-dependent correctness

## Failure model

- TooManyNodes -> truncate at cap; TooDeep -> placeholder; MeasureFailed -> error, no zero-width fallback; Cache exceeded -> LRU evict, recompute

## Proof / validation strategy

```bash
cargo test -p nexus-layout -- --nocapture              # Phase 0+1: unit tests
cargo test -p ui_v3a_host -- --nocapture                # Phase 3: JSON goldens
cargo test -p ui_v3a_host golden -- --nocapture         # Phase 3: PNG goldens
UPDATE_GOLDENS=1 cargo test -p ui_v3a_host              # Golden regeneration
RUN_UNTIL_MARKER=1 just test-os visible-bootstrap       # Phase 4: QEMU
```

Markers: `layout: engine on`, `text: wrapping on`, `SELFTEST: ui v3 wrap ok`

## Alternatives considered

- VStack/HStack -> rejected (DSL uses `Stack { direction }`)
- Float math -> rejected (non-deterministic)
- Full CSS grid auto-placement -> deferred
- nexus-shape embedded -> rejected (MeasureText trait decouples)
- Theme tokens as enum in layout -> rejected (Rgba8 in consumer layer keeps layout pure)
- VisualStyle inline -> rejected (separate struct enforces paint-only invalidation)

---

## Implementation Checklist

- [ ] **Phase 0 (Container layout)**: `Stack`/`Grid`/`Spacer` + `FlexItem` + `FxPx`/`EdgeInsets` + `Direction`/`Align`/`Justify`/`Overflow`/`Position`/`ZIndex` + flex/grid algorithms -- proof: `cargo test -p nexus-layout`
- [ ] **Phase 1 (Visual + Text primitives)**: `Rgba8`/`Border`/`EdgeBorder`/`CornerRadius`/`VisualStyle` + `TextAlign`/`LineHeight`/`FontWeight`/`WhiteSpace`/`TextStyle`/`TextNode` + `MeasureText` trait -- proof: `cargo test -p nexus-layout`
- [ ] **Phase 2 (Text wrapping + caches)**: `wrap.rs` -- UAX#14 subset + ellipsis + paragraph/run cache + line-layout cache -- proof: `cargo test -p nexus-shape wrap`
- [ ] **Phase 3 (Host tests)**: `tests/ui_v3a_host/` -- JSON goldens (layout boxes + VisualStyle) + PNG goldens (rendered with backgrounds/borders/text) -- proof: `cargo test -p ui_v3a_host`
- [ ] **Phase 4 (windowd integration)**: proof panel driven by layout engine with theme-resolved colors -- proof: `RUN_UNTIL_MARKER=1 just test-os visible-bootstrap`
- [ ] Security negative tests (node count, depth, div-by-zero)
- [ ] QEMU markers in `scripts/qemu-test.sh` verified