# Plan: Generic Panel + VirtualList on the layout engine — remove "chat" from windowd

## Context

windowd has a chat baked into the compositor (`chat_provider`, `compositor/chat.rs::compute_visible`
+ `draw_chat_panel_row`, `chat_scroll_y`/`chat_content_h`/`chat_visible` on `DisplayServerRuntime`,
a chat window in `wm.rs`, a chat button). This is wrong on three counts the user identified:

1. **windowd should not know about "chat".** It should host a generic **panel + virtual list**, and
   "chat" should be an app/target-test *configuration* of that — Apple's model: the framework ships
   the collection view + layout, the app supplies a data source + cell config.
2. **There are three parallel list/measurement implementations** and the live path uses the worst:
   - `VirtualList<P: ItemProvider>` (`userspace/ui/widgets/virtual_list/`) — lazy loading
     (`request_more`/`has_inflight`/`get→&[Option<Item>]`), recycling, overscan, scene-graph node
     ids — **but windowd never uses it**.
   - `compositor/chat.rs::compute_visible` (LIVE) — **O(all messages) per scroll**
     (`provider.get(0..len)`), no lazy load, CPU-drawn into an atlas. This is the perf bug
     ("UI updated alle paar Sekunden").
   - `nexus_layout` (the pretext layout engine, RFC-0057) — used **only** for the proof panel.
3. **VirtualList should use our layout** (`nexus_layout`) as the single measurement SSOT, not its
   own `MeasuredRow`.

The good news: the **correct path already exists for the proof panel** —
`layout_panel.rs::build_combined_tree(state) -> LayoutNode` → `LayoutEngine::layout` →
`LayoutResult { boxes }` → `compositor/surface.rs::draw_row_box(&LayoutBox)` (generic per-box paint
from `VisualStyle`). The chat just bypasses it. The redesign routes **all lists through that
existing generic path, virtualized**, and moves the chat out of windowd.

## Target architecture (Apple-style)

One render contract, no widget-specific draw code in windowd:

```
data source (ItemProvider, lazy)  +  item builder (index,&Item) -> LayoutNode
        │                                        │
        └────────── VirtualList<P> ──────────────┘   (windows the collection)
                          │ builds LayoutNodes for visible+overscan items only
                          ▼
                 LayoutEngine::layout  (nexus_layout = the one measurement SSOT)
                          ▼
                 LayoutResult { boxes: Vec<LayoutBox> }  + compute_scroll_damage
                          ▼
        windowd: draw_row_box(&LayoutBox) / scene-graph nodes   (generic, already exists)
```

- **windowd = generic compositor.** Hosts a scrollable **Panel** containing a **VirtualList** driven
  by a data-source + item-builder *interface*. It has **no `chat` symbols**.
- **VirtualList uses `nexus_layout`** for measurement/placement (drop `MeasuredRow`): it builds
  `LayoutNode`s only for the visible+overscan range, lays them out, paints via `draw_row_box`;
  `content_height` + scroll damage come from the engine; lazy loads via `ItemProvider`.
- **Data-source interface = the "Schnittstelle für Variablen"** (Apple's `DataSource` + cell): the
  app implements `ItemProvider` (data) + an item→`LayoutNode` builder (cell). State flows through it.
- **Chat = a target-test/app module**: `ChatMessageProvider` (exists) + a chat item-builder
  (`ChatMessage` → bubble `LayoutNode`). Assembled into panel+VirtualList in the target test. windowd
  is unaware it is "a chat".

## Phases (each host-tested + boot-verified before the next)

### Phase 1 — VirtualList on `nexus_layout` + the item-builder interface
- Extend the data-source contract: keep `ItemProvider` (data/lazy) and add an item→`LayoutNode`
  builder (a trait method or companion `ItemView` trait: `fn item_node(&self, index, &Item) -> LayoutNode`).
- Rework `VirtualList<P>` to measure/place via `LayoutEngine::layout` over the **visible+overscan**
  items' `LayoutNode`s (delete `MeasuredRow`); expose the visible `LayoutBox`es, `content_height`,
  and `scroll_by` → `compute_scroll_damage`. Keep recycling + lazy `request_more`.
- Crate: `userspace/ui/widgets/virtual_list/`. Reuse `nexus_layout::{LayoutEngine, LayoutResult,
  compute_scroll_damage}` + `MeasureText`. Host tests: visible range is O(window) not O(N); scroll
  yields ≤2 damage rects; lazy `request_more` fires at the overscan edge.

### Phase 2 — windowd: generic scrollable-list render
- Add a generic "render a `VirtualList`'s visible `LayoutBox`es inside a clipped scroll panel" path,
  reusing `compositor/surface.rs::draw_row_box` + `compute_scroll_damage` for O(delta) scroll blits.
  No item-type knowledge in windowd.
- `DisplayServerRuntime` holds a generic `VirtualList<dyn …>`/`VirtualList<P>` handle, not chat fields.

### Phase 3 — Externalize the chat as an app/target-test config
- New module (target-test/app side): chat = `ChatMessageProvider` + a `ChatMessage`→`LayoutNode`
  item builder (bubble: rounded rect `VisualStyle` + text node). The target test assembles
  panel + VirtualList + this data source/builder.

### Phase 4 — Delete chat from windowd
- Remove `compositor/chat.rs` (`compute_visible`, `draw_chat_panel_row`, `ChatVisibleMsg`), the
  `chat_*` fields/methods on `DisplayServerRuntime` (`chat_provider`, `chat_scroll_y`,
  `chat_content_h`, `chat_visible`, `render_chat_surface`, `handle_chat_scroll_input`,
  `on_chat_window_closed`, `note_chat_window_moved`, `erase_chat_region`, `note_chat_button_dirty`),
  and the ad-hoc chat measurement (`chat_message_height`/`chat_message_lines`). windowd grep for
  "chat" → only generic panel/list/data-source.

### Phase 5 — Converge with the scene-graph composite (#23)
- The list's `LayoutBox`es become scene-graph nodes (one node per visible item, recycled); the
  damage-driven composite (`generate_commands_for_damage`, already added) paints only changed boxes.
  Animation/scroll = node property + `compute_scroll_damage`. This unifies #20/#21/#22/#23/#24.

## Critical files
- `userspace/ui/widgets/virtual_list/src/lib.rs` — VirtualList on `nexus_layout`; item-builder trait.
- `userspace/ui/layout/src/engine.rs` — reuse `LayoutEngine`/`LayoutResult`/`compute_scroll_damage` (no change expected).
- `source/services/windowd/src/compositor/surface.rs` — reuse `draw_row_box` as the generic box painter.
- `source/services/windowd/src/layout_panel.rs` — model for building `LayoutNode` trees (the proof panel already does this right).
- `source/services/windowd/src/compositor/runtime/mod.rs` + `compositor/chat.rs` — remove chat (Phase 4).
- target-test/app module (new) — the chat data source + item builder.

## Verification
- **Host:** `cargo test -p nexus-virtual-list` (O(window) visible range; lazy `request_more`; scroll
  damage ≤2 rects), `cargo test -p windowd` (generic list render; no chat symbols after Phase 4),
  `cargo test -p nexus-layout`. `cargo check` riscv os-lite for windowd + virtual_list.
- **QEMU (user, mmio CPU):** the target-test panel scrolls **smoothly** (O(window), not O(N));
  `fps: windowd` `damage_px` small per scroll; cursor stays smooth (HW). Regression: panel/sidebar/
  buttons unchanged; mmio safety boot clean.

## Notes / sequencing
- Orthogonal cleanup already in flight: the `runtime.rs` monolith split into `runtime/{gpud,cursor,
  anim,marker_emit}.rs` (4 clusters done, mod.rs 3642→2986, all green) — genuine runtime concerns;
  continue opportunistically. The chat methods stay in `mod.rs` until Phase 4 deletes them (do NOT
  extract chat into a `runtime/` submodule — it leaves windowd entirely).
- Open design knob locked by this plan: **measurement SSOT = `nexus_layout`** (VirtualList drops
  `MeasuredRow`). The proof panel already validates this path.
- Big multi-phase redesign → land phase-by-phase behind host tests + a boot at each phase. Do NOT
  touch the virgl GL-scanout black-screen (separate/deferred); all phases are mode-independent.
