# Next Task Prep — TASK-0058

After TASK-0057: TASK-0058 (UI v3a: layout wrapping + deterministic box model).

## Current status

RFC-0057 contract seed: In Progress (complete, implementation pending).
TASK-0058: In Progress. TASK-0059: Draft (depends on TASK-0058).

## Drift check

- TASK-0058 depends-on TASK-0057 (text shaping, SVG pipeline) — DONE
- TASK-0058 depends-on TASK-0056 (present/input baseline) — DONE
- nexus-shape (rustybuzz + fontdue) available for text measurement
- nexus-theme available for token resolution (consumer layer, not layout crate)
- windowd proof panel exists at hardcoded coordinates — ready for layout replacement
- No kernel changes expected
- Contract: RFC-0057 defines all types, phases, proof gates

## Layout engine type surface

- Containers: Stack (flex row/col + flex_wrap), Grid (fraction cols + row_gap/col_gap), Spacer
- Flex children: FlexItem (grow, shrink, align_self, margin, position, z_index)
- Visual: Rgba8, Border, EdgeBorder, CornerRadius, VisualStyle (paint-only)
- Text: TextStyle (font_size, weight, line_height, text_align, color, white_space), TextAlign, LineHeight, FontWeight, WhiteSpace
- Constraints: min/max_width, min/max_height, Overflow
- Measurement: MeasureText trait (decoupled from nexus-shape)

## Implementation plan (per RFC-0057)

- Phase 0: Container layout — Stack/Grid/Spacer + FxPx/EdgeInsets + flex/grid algorithms
- Phase 1: Visual + Text primitives — Rgba8/Border/VisualStyle + TextStyle/MeasureText
- Phase 2: Text wrapping + caches — UAX#14, ellipsis, paragraph/run + line-layout caches
- Phase 3: Host tests — JSON + PNG goldens (tests/ui_v3a_host/)
- Phase 4: windowd integration — proof panel replacement + markers

## Immediate follow-ups

- TASK-0059 (clip/scroll/effects + IME/TextInput) — consumes v3a layout tree
- TASK-0062 (reactive runtime + animation/transitions)
- TASK-0063 (virtualized list + theme tokens)
