# Next Task Prep — TASK-0059

After TASK-0058: TASK-0059 (UI v3b: clip/scroll/effects + IME stub + filter-box).

## Current status

RFC-0058 contract seed: In Progress (complete, implementation pending).
TASK-0059: In Progress. Depends on TASK-0058 (DONE).

## Drift check

- TASK-0059 depends-on TASK-0058 (layout engine) — DONE
- nexus-layout (Flex/Grid) available for clip rect derivation
- nexus-layout-types available for TextInputNode type
- windowd layout_panel.rs available for filter-box layout tree
- ProofPaintRole system available for allocation-free rendering
- No kernel changes expected

## Filter-box proof element

Integration test for all three v3b features:
- Clip: Overflow::Hidden container for scrollable word list
- Scroll: wheel/drag → viewport, place-only invalidation, scrollbar
- IME: TextInput node receives keyboard events, cursor blink via effect timer
- Filter: filter_words(prefix) pure function on 15-word static list

## Implementation plan (per RFC-0058)

- Phase 0: Clip + scroll — scissor clipping, scroll damage math, scrollbar
- Phase 1: Text input + filter-box — TextInputNode, filter_words(), layout, routing
- Phase 2: CPU effects — blur/shadow + budgets, cursor blink timer
- Phase 3: IME stub — focus routing, caret/selection helpers
- Phase 4: Host tests — tests/ui_v3b_host/
- Phase 5: OS markers — 12 new markers + postflight

## Immediate follow-up

- TASK-0060B (glass materials — consumes effect primitives)
