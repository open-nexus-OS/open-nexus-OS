# Handoff — TASK-0059 **In Progress** (Phases 0-4 Done, Phase 5 pending)

Date: 2026-05-18

## Status

- RFC-0058: Phases 0-4 ✅, Phase 5 ⬜ (markers defined, wiring pending)
- TASK-0059: Phases 0-4 complete. Phase 5 (OS marker wiring + QEMU proof) pending.
- Depends on: TASK-0058 (DONE)
- Follow-up: TASK-0060B (glass materials)

## What was delivered

### Phase 0: Clip + Scroll (cores/layout engine)
- `LayoutBox` extended: `clip_rect`, `scroll_offset`, `overflow` fields
- `Overflow::Hidden` containers: scissor clip rect = container content rect; children inherit
- `compute_scroll_damage(old, new, viewport) -> ScrollDamage (≤2 rects)`: deterministic, integer-only
- `LayoutResult::reposition_scroll(container_id, new_offset) -> ScrollDamage`: mutates existing boxes (allocation-free), shifts children by delta

### Phase 1: TextInput + Filter-Box (layout-types + windowd)
- `TextInputNode` type + `LayoutNode::TextInput` variant: content, cursor_pos, placeholder, max_length
- Measures like TextNode (placeholder used when content empty)
- `filter_words(prefix) -> Vec<&str>`: case-insensitive filter, 15-word static list
- Filter-box in `layout_panel.rs`: TextInput + `Overflow::Hidden` scrollable filtered word list
- Proof panel: 3 cards (hover/click/key) in Column, filter-box in right Column, panel 640×290

### Phase 2: Effects (nexus-effects crate)
- `blur.rs`: `blur_3x3`, `blur_1x3_horizontal` (integer-only, premultiplied alpha)
- `shadow.rs`: `composite_drop_shadow(target, alpha_mask, offset, color, budget)`
- `budget.rs`: `EffectBudget` with `try_reserve`, `reset`, `fraction` for deterministic degrade
- `cache.rs`: `EffectCache` (LRU, fixed capacity, allocation-free after construction)
- `cursor_blink.rs`: `CursorBlink` (frame-count based toggle, default 30-frame interval)

### Phase 3: IME Stub (imed service)
- `source/services/imed/`: `ImedService`, `TextFocus`, `CaretSelection`
- Focus routing: `set_focus(surface_id)`, `clear_focus()`
- Caret: `move_caret(text_len, delta)` (clamped), `set_selection(anchor, caret, text_len)`
- Marker: `imed: ready`
- 6 unit tests

### Phase 4: Host Tests (tests/ui_v3b_host)
- 23 tests: scroll damage (5), clip (2), filter_words (6), filter-box layout (3), scroll reposition (1), effects budget (3), blur (2), cursor blink (2)

## Files changed

### New
- `userspace/ui/effects/` (crate)
- `source/services/imed/` (service)
- `tests/ui_v3b_host/` (test crate)

### Modified
- `userspace/ui/layout/src/engine.rs` (LayoutBox + clip/scroll/TextInput)
- `userspace/ui/layout/src/lib.rs`
- `userspace/ui/layout-types/src/node.rs` (TextInputNode)
- `userspace/ui/layout-types/src/lib.rs`
- `source/services/windowd/src/layout_panel.rs` (filter-box)
- `source/services/windowd/src/proof_panel_spec.rs` (filter_words, fix)
- `source/services/windowd/src/markers.rs` (12 new markers)
- `source/services/windowd/src/lib.rs` (exports)
- `source/services/windowd/src/os_lite.rs` (signature update)
- `tests/ui_v3a_host/src/lib.rs` (signature updates, scroll-card removal)
- `Cargo.toml` (workspace)
- `CHANGELOG.md`
- `docs/rfcs/RFC-0058-*.md` (checklist/status)

## Proof

```bash
# All host tests pass:
cargo test -p nexus-layout       # 8/8
cargo test -p windowd            # 29/29
cargo test -p imed               # 6/6
cargo test -p ui_v3a_host        # 10/10
cargo test -p ui_v3b_host        # 23/23
just dep-gate                    # PASS
```

## Pending (Phase 5)

1. Wire 12 OS markers in `os_lite.rs` render path (marker emissions at: layout compute, clip application, scroll operation, filter list render, effects pipeline)
2. Add markers to `source/apps/selftest-client/proof-manifest/markers/ui.toml`
3. Wire keyboard routing: inputd → windowd focused surface → filter-box TextInput
4. QEMU proof: `RUN_UNTIL_MARKER=1 just test-os visible-bootstrap`

## Next step

Wire the 12 OS markers in `os_lite.rs` and the marker manifest. Then QEMU visible-bootstrap proof.
