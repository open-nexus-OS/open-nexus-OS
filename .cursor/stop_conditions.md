# Stop Conditions — TASK-0059

## Hard stop (do not claim done without)

- [ ] Scissor clipping via `Overflow::Hidden` on layout containers
- [ ] Scroll damage math: viewport delta → dirty rect set (order-agnostic)
- [ ] Scrollbar affordance: visible thumb + track with hover/active states
- [ ] Scroll = place-only: no text reshaping or layout remeasurement on scroll
- [ ] `TextInputNode` type in `nexus-layout-types`
- [ ] `filter_words(prefix)` pure function with 15-word static list
- [ ] Filter-box proof element visible on proof surface
- [ ] Keyboard → text input routing in windowd
- [ ] Filtered word list updates in real-time on keystroke
- [ ] Cursor blink via effect timer
- [ ] CPU blur + drop shadow with deterministic budgets
- [ ] Effect budget trip degrades deterministically (marker emitted)
- [ ] IME/text-input stub: focus routing, caret/selection helpers
- [ ] `imed` stub (or real `imed` if TASK-0147 present)
- [ ] Host tests: `tests/ui_v3b_host/` — scroll, clip, effects, filter, IME
- [ ] JSON goldens stable
- [ ] 12 OS markers fire: clipping, scroll, live scroll, text input, filter list, effects, blur, imed, SELFTEST ui v3 scroll/ime/effect/filter ok

## Reject tests required

- [ ] test_reject_clip_outside_bounds
- [ ] test_reject_scroll_overflow
- [ ] test_scroll_place_only (no remeasure on scroll)
- [ ] test_effect_budget_exceeded (degrade + marker)
