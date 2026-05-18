# Current State — Open Nexus OS

Last updated: 2026-05-18 (TASK-0059 phases 0-4 Done, 76 tests green, dep-gate PASS)

## Active task

TASK-0059: UI v3b clip/scroll/effects + IME stub + filter-box. — **In Progress**
Status: Phases 0-4 complete. Phase 5 (OS marker wiring) pending.
RFC-0058: Phases 0-4 checked, Phase 5 pending.
Depends on: TASK-0058 (DONE).

## What TASK-0059 delivered

### Phase 0: Clip + Scroll
- `LayoutBox` extended with `clip_rect: Option<Rect>`, `scroll_offset: (FxPx, FxPx)`, `overflow: Overflow`
- `Overflow::Hidden` containers propagate scissor rects to children
- `compute_scroll_damage()`: bounded (≤2 rects), allocation-free, deterministic
- `LayoutResult::reposition_scroll()`: place-only reposition (no remeasure), shifts children by scroll delta

### Phase 1: TextInput + Filter-Box
- `TextInputNode` type: content, cursor_pos, placeholder, max_length; added to `LayoutNode` enum
- `filter_words(prefix) -> Vec<&str>`: case-insensitive filter on 15-word static list
- Filter-box layout tree: TextInput + `Overflow::Hidden` scrollable filtered word list
- Proof panel restructured: 3 cards (hover/click/key) in vertical column; filter-box in right column
- Panel dimensions: 640×290 (was 610×260)

### Phase 2: Effects
- New `nexus-effects` crate (`userspace/ui/effects/`): `blur`, `shadow`, `budget`, `cache`, `cursor_blink`
- `blur_3x3` and `blur_1x3_horizontal`: integer-only box blur
- `composite_drop_shadow`: offset, alpha mask, blur, composite onto target
- `EffectBudget`: per-frame pixel cap with `try_reserve` and deterministic degrade
- `EffectCache`: LRU cache with fixed capacity
- `CursorBlink`: frame-count-based caret blink timer

### Phase 3: IME Stub
- New `imed` service (`source/services/imed/`): `ImedService`, `TextFocus`, `CaretSelection`
- Focus routing: `set_focus(surface_id)`, `clear_focus()`
- Caret movement clamped to text length; selection range helpers
- 6 unit tests, `imed: ready` marker constant

### Phase 4: Host Tests
- New `tests/ui_v3b_host/` crate: 23 tests
- Scroll damage: empty delta, down/up, horizontal
- Clip: Overflow::Hidden sets clip_rect, Visible passes through
- filter_words: exact match, case-insensitive, empty, no match, prefix
- Filter-box: layout contains TextInput + filter_list, child count matches filter
- Scroll reposition: shifts children, produces damage
- Effects: budget reserve/exhaust/reset/fraction, blur identity, cursor blink toggle/reset

## Files changed/created

### New files
- `userspace/ui/effects/Cargo.toml`, `src/lib.rs`, `src/blur.rs`, `src/shadow.rs`, `src/budget.rs`, `src/cache.rs`, `src/cursor_blink.rs`
- `source/services/imed/Cargo.toml`, `src/lib.rs`, `src/main.rs`
- `tests/ui_v3b_host/Cargo.toml`, `src/lib.rs`

### Modified files
- `userspace/ui/layout/src/engine.rs`: LayoutBox fields, clip propagation, scroll offset, reposition_scroll, TextInput handling
- `userspace/ui/layout/src/lib.rs`: ScrollDamage export
- `userspace/ui/layout-types/src/node.rs`: TextInputNode type, LayoutNode::TextInput variant
- `userspace/ui/layout-types/src/lib.rs`: TextInputNode export
- `source/services/windowd/src/layout_panel.rs`: filter-box layout tree, build_filter_box(), updated panel dimensions
- `source/services/windowd/src/proof_panel_spec.rs`: filter_words(), FILTER_WORDS, duplicate copyright fix
- `source/services/windowd/src/markers.rs`: 12 new v3b markers
- `source/services/windowd/src/lib.rs`: new exports (filter_words, FILTER_WORDS, markers)
- `source/services/windowd/src/os_lite.rs`: compute_proof_layout signature update
- `tests/ui_v3a_host/src/lib.rs`: signature updates, TextInput match arm, scroll-card test updated
- `Cargo.toml`: workspace members (nexus-effects, ui_v3b_host)
- `CHANGELOG.md`: TASK-0059 entry
- `docs/rfcs/RFC-0058-ui-v3b-clip-scroll-effects-ime-contract.md`: checklist + status updates

## Proofs

```bash
cargo test -p nexus-layout       # 8/8
cargo test -p nexus-layout-types # (0 doc tests)
cargo test -p nexus-effects      # (0 doc tests)
cargo test -p windowd            # 29/29
cargo test -p imed               # 6/6
cargo test -p ui_v3a_host        # 10/10
cargo test -p ui_v3b_host        # 23/23
just dep-gate                    # PASS
```

## Pending

### Phase 5: OS marker wiring
- Wire 12 new markers in `os_lite.rs` render path
- Add markers to `selftest-client/proof-manifest/markers/ui.toml`
- QEMU visible-bootstrap proof: `RUN_UNTIL_MARKER=1 just test-os visible-bootstrap`
- Keyboard routing wire-up (inputd → windowd → filter-box TextInput)

## Architecture decisions

| Decision | Rationale |
|----------|-----------|
| Clip rects ARE layout boxes | No separate clip tree; `Overflow::Hidden` on container → clip_rect = content rect |
| Scroll = place-only | Layout boxes from v3a reused; no remeasure or text reshape on scroll |
| filter_box_right column | Added to right of cards; hover/click/key cards in vertical column; scroll card replaced by actual scrollable list |
| Effects as separate crate | Independent of layout engine; consumed by windowd renderer |
| imed as new service | Separate from `ime` (TASK-0253); focus routing + caret helpers |
| Bump-allocator safety | `reposition_scroll` mutates existing boxes, no allocation |
