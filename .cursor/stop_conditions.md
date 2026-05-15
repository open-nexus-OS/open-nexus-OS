# Stop Conditions — TASK-0058

## Hard stop (do not claim done without)

- [ ] `userspace/ui/layout/` crate exists with all types from RFC-0057
- [ ] `FxPx` fixed-point type, `Rect`, `EdgeInsets` implemented
- [ ] `Stack` (flex row/col + flex_wrap), `Grid` (fraction cols + row_gap), `Spacer` working
- [ ] `FlexItem` (grow, shrink, align_self, margin, position, z_index) working
- [ ] `Rgba8`, `Border`, `EdgeBorder`, `CornerRadius`, `VisualStyle` types
- [ ] `TextStyle`, `TextAlign`, `LineHeight`, `FontWeight`, `WhiteSpace` types
- [ ] `MeasureText` trait defined and implemented in nexus-shape
- [ ] Flex algorithm deterministic (grow/shrink, space-between, align-items)
- [ ] Grid algorithm deterministic (fraction columns, gap)
- [ ] Text wrapping: UAX#14 minimal subset working
- [ ] Ellipsis and max-lines truncation working
- [ ] Paragraph/run cache + line-layout cache split working
- [ ] JSON goldens stable (layout boxes + VisualStyle)
- [ ] PNG goldens stable (rendered with backgrounds, borders, text)
- [ ] windowd proof panel driven by layout engine
- [ ] Proof panel pixel-identical regression gate maintained
- [ ] OS markers: `layout: engine on`, `text: wrapping on`
- [ ] All host tests green (`nexus-layout`, `nexus-shape wrap`, `ui_v3a_host`)

## Reject tests required

- [ ] test_reject_too_many_nodes
- [ ] test_reject_too_deep
- [ ] test_reject_div_by_zero_flex
- [ ] test_reject_oversized_text
- [ ] test_place_only_no_remeasure (scroll = place-only invalidation)
- [ ] test_visual_style_no_remeasure (VisualStyle change = paint-only)
