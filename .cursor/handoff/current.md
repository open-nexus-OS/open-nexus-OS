# Handoff — TASK-0059 **In Progress** (RFC-0058 contract seed created)

Date: 2026-05-17

## Status

- RFC-0058: **In Progress** — contract seed complete, implementation pending
- TASK-0059: **In Progress** — filter-box integration test defined, 5 PRs scoped
- Depends on: TASK-0058 (DONE)
- Follow-up: TASK-0060B (glass materials)

## Filter-box proof element

Integration test exercising all three v3b features:
- **Clip**: `Overflow::Hidden` container for scrollable word list
- **Scroll**: Wheel/drag → viewport scroll, place-only invalidation, scrollbar affordance
- **IME**: `TextInput` node receives keyboard events, cursor blink via effect timer
- **Filter**: `filter_words(prefix)` pure function on 15-word static list

## Implementation plan (per RFC-0058)

1. Clip + scroll: scissor clipping, scroll damage math, scrollbar
2. Text input + filter-box: TextInputNode, filter_words(), filter-box layout, keyboard routing
3. CPU effects: blur/shadow + budgets, cursor blink timer
4. IME stub: focus routing, caret/selection helpers, imed stub
5. Proof + docs: host tests + OS markers + postflight

## OS markers (12 new)

`windowd: clipping on`, `windowd: scroll on`, `windowd: live scroll ok`,
`windowd: text input on`, `windowd: filter list ok`, `windowd: effects on`,
`windowd: effect blur ok`, `imed: ready`, `SELFTEST: ui v3 scroll ok`,
`SELFTEST: ui v3 ime ok`, `SELFTEST: ui v3 effect ok`, `SELFTEST: ui v3 filter ok`

## Key decisions

| Decision | Rationale |
|----------|-----------|
| Filter-box as integration test | One element tests clip+scroll+IME+effects |
| `filter_words()` as pure function | No IME needed, testable host-first |
| Scroll = place-only | Layout boxes from TASK-0058 reused, no remeasure |
| Clip rects ARE layout boxes | No separate clip tree, `Overflow::Hidden` on container |
| Bump-allocator safety | Layout computation in `new()` only, scroll damage allocation-free |

## Previous task archive

`.cursor/handoff/archive/TASK-0058-20260517.md`
