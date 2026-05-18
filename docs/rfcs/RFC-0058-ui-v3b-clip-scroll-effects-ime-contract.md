# RFC-0058: UI v3b clipping/scroll/effects + IME/text-input contract seed

- Status: In Progress
- Last Updated: 2026-05-17
- Owners: @ui
- Created: 2026-05-17
- Links:
  - Tasks: `tasks/TASK-0059-ui-v3b-clip-scroll-effects-ime-textinput.md` (execution + proof)
  - Depends on: `docs/rfcs/RFC-0057-ui-v3a-layout-engine-pretext-contract.md` (layout engine)
  - Follow-up: `tasks/TASK-0060B-ui-v4b-glass-materials-backdrop-cache-degrade.md`
  - Layout contract: `docs/rfcs/RFC-0057-ui-v3a-layout-engine-pretext-contract.md`
  - Layout pipeline: `docs/dev/ui/foundations/layout/layout-pipeline.md`
  - Scroll spec: `docs/dev/ui/foundations/layout/scroll.md`
  - Architecture: `docs/architecture/display-output-service-chain.md`

## Status at a Glance

- **Phase 0 (Clip + scroll)**: ✅ — scissor clipping, scroll damage math, scrollbar affordance
- **Phase 1 (Text input + filter-box)**: ✅ — TextInputNode, filter_words(), filter-box proof element
- **Phase 2 (CPU effects)**: ✅ — blur/shadow, budgets, cursor blink timer
- **Phase 3 (IME/text-input stub)**: ✅ — focus routing, caret/selection, imed stub
- **Phase 4 (Host tests)**: ✅ — JSON + PNG goldens
- **Phase 5 (OS markers + postflight)**: ⬜ — QEMU markers defined, wiring pending

## Scope boundaries

- **This RFC owns**: scissor clipping via `Overflow::Hidden`, scroll damage math (viewport delta → dirty rects), scrollbar affordance (thumb + track), CPU blur/drop-shadow with budgets, TextInputNode type, keyboard → text routing, filter-box proof element, cursor blink via effect timer, IME focus-routing stub
- **This RFC does NOT own**: full IME engine (TASK-0146/0147), keymaps/OSK, clipboard, GPU effects, kernel changes

## Context

TASK-0058 delivered a deterministic layout engine with `LayoutResult { boxes, content_height }`.
v3b builds on this: scroll operates on the stable layout tree (place-only invalidation), clip
rects are derived from layout box coordinates, and effects add visual polish within budgets.

The **filter-box** is the integration test: one proof element that exercises clip (Overflow::Hidden),
scroll (wheel/drag → viewport), IME (keyboard → text input), and effects (cursor blink).

## Goals

1. Scissor clipping via `Overflow::Hidden` on layout containers
2. Scroll damage math: viewport delta → dirty rect set, place-only invalidation
3. Scrollbar affordance: visible thumb + track with hover/active states
4. CPU blur + drop shadow with deterministic budgets and degrade
5. `TextInputNode` type + keyboard → text routing
6. `filter_words(prefix)` pure function for real-time word filtering
7. Filter-box proof element integrating goals 1+2+5+6
8. Cursor blink via effect timer
9. IME/text-input stub: focus routing + caret/selection helpers
10. Host tests + OS markers

## Non-Goals

Full IME engine; keymaps/compose tables; OSK overlay; clipboard; GPU effects; text reshaping during scroll.

## Constraints

- Deterministic damage math (integer-only, order-agnostic rect comparison)
- Scroll = place-only: no text reshaping or layout remeasurement on scroll
- Bump-allocator safety: layout computation only in `new()`, scroll damage allocation-free
- Effect budgets: cap blur radius/area per frame, LRU eviction for cached effects
- No `unwrap/expect`

## Proposed design

### Clip + scroll

``` text
layout_box.overflow == Hidden → scissor rect = layout_box.rect
scroll_offset = (dx, dy) → viewport = layout_box.rect + scroll_offset
scroll_delta → dirty_rects = old_viewport ∪ new_viewport
```

Clip rects ARE layout boxes — no separate clip tree. When a container has `Overflow::Hidden`,
all children are clipped to its content rect.

### Filter-box proof element

``` text
Layout tree:
  Stack(Row) [filter_box_row]
  ├── Stack(Column) [cards_left]  ← existing hover/click/key cards
  └── Stack(Column) [filter_box_right]
      ├── Stack(Row) [filter_input]  ← TextInput + label
      │   └── TextInput { content, cursor_pos, max_length }
      └── Stack(Column, overflow: Hidden) [filter_list]
          ├── Text("apple")
          ├── Text("application")
          └── Text("apt")
```

`filter_words(prefix: &str) -> Vec<&str>` filters a static word list:
```rust
const FILTER_WORDS: &[&str] = &[
    "apple", "application", "apt", "arrow", "asset",
    "batch", "binary", "block", "buffer", "build",
    "cache", "clock", "compile", "component", "config",
];
fn filter_words(prefix: &str) -> Vec<&str> { ... }
```

### Invalidation matrix

| Change | Class | Work |
|--------|-------|------|
| scroll offset | `place-only` | reclip, reposition scrollbar |
| filter text change | `measure+place` | redo filter + list layout |
| cursor blink tick | `paint-only` | repaint cursor area |
| theme change | `paint-only` | repaint |

## Security

- IME focus scoped to focused surface only; policy can deny IME
- Effect budgets prevent memory exhaustion
- Scroll damage math bounded (no unbounded dirty region)
- No heap allocation in input hot-path

## Proof

### Host
```bash
cargo test -p ui_v3b_host -- --nocapture
```

### OS/QEMU
```bash
RUN_UNTIL_MARKER=1 just test-os visible-bootstrap
```

Markers: `windowd: clipping on`, `windowd: scroll on`, `windowd: live scroll ok`,
`windowd: text input on`, `windowd: filter list ok`, `windowd: effects on`,
`windowd: effect blur ok`, `imed: ready`, `SELFTEST: ui v3 scroll ok`,
`SELFTEST: ui v3 ime ok`, `SELFTEST: ui v3 effect ok`, `SELFTEST: ui v3 filter ok`

---

## Implementation Checklist

- [x] **Phase 0 (Clip + scroll)**: `Overflow::Hidden` → scissor, scroll damage math, scrollbar — proof: `cargo test -p ui_v3b_host`
- [x] **Phase 1 (Text input + filter-box)**: `TextInputNode`, `filter_words()`, filter-box layout, keyboard routing — proof: `cargo test -p ui_v3b_host`
- [x] **Phase 2 (CPU effects)**: blur/shadow + budgets, cursor blink — proof: `cargo test -p ui_v3b_host`
- [x] **Phase 3 (IME stub)**: focus routing, caret/selection helpers, imed stub — proof: `cargo test -p ui_v3b_host`
- [x] **Phase 4 (Host tests)**: JSON + PNG goldens — proof: `cargo test -p ui_v3b_host`
- [ ] **Phase 5 (OS markers)**: QEMU markers wired + postflight — proof: `RUN_UNTIL_MARKER=1 just test-os visible-bootstrap`
