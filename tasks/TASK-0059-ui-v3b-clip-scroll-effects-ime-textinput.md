---
title: TASK-0059 UI v3b: clipping/scroll layers + precise damage + CPU effects (blur/shadow) + IME/text-input stub
status: Done
owner: @ui
created: 2025-12-23
updated: 2026-05-22 (Phases 0–7 complete. ShadowArena + per-box caching + zero-alloc blur in production. `os_lite.rs` (4860 lines) → `compositor/` (18 focused files). All tests green, no functional change.)
depends-on: [TASK-0058]
follow-up-tasks: [TASK-0060B]
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - **RFC seed (SSOT contract)**: docs/rfcs/RFC-0058-ui-v3b-clip-scroll-effects-ime-contract.md
  - **ADR-0030**: docs/adr/0030-layout-engine-deterministic-pretext.md
  - UI v3a layout: tasks/TASK-0058-ui-v3a-layout-wrapping-deterministic.md
  - Layout pipeline: docs/dev/ui/foundations/layout/layout-pipeline.md
  - Architecture: docs/architecture/display-output-service-chain.md
  - Testing: scripts/qemu-test.sh
---

## Context

TASK-0058 delivered a production-grade layout engine: `nexus-layout-types`, `nexus-layout`
(Flex/Grid), `nexus-shape` wrap.rs + cache.rs, and `windowd` integration via `layout_panel.rs`
with the `ProofPaintRole` system.

v3b adds clipping, scroll damage, CPU effects, and a minimal IME stub. To exercise all three
features in one coherent test surface, v3b introduces a **filter-box proof element** on the
shared proof panel.

### Integration test: filter-box

``` text
┌──────────────────────────────────────────────────────┐
│  ┌─────┐ ┌─────┐ ┌─────┐  ┌──────────────────┐      │
│  │hover│ │click│ │ key │  │ type to filter…  │      │  ← TextInput
│  │ 126 │ │ 126 │ │ 126 │  └──────────────────┘      │
│  └─────┘ └─────┘ └─────┘  ┌──────────────────┐      │
│                            │ apple     ▲      │      │  ← Scrollbare
│                            │ application██   │      │     Wortliste
│                            │ apt       ██      │      │
│                            │ arrow     ██      │      │
│                            │ asset     ▼      │      │
│                            └──────────────────┘      │
└──────────────────────────────────────────────────────┘
```

- **TextInput**: Keyboard events via `inputd` → `windowd` focus → text. Cursor blink via effect timer.
- **Filter**: `filter_words(prefix) → Vec<&str>` pure function, triggers on each keystroke.
- **Scroll**: `Overflow::Hidden` container. Wheel/drag → viewport scroll. Place-only invalidation.
- **Scrollbar**: Thumb + track, hover/active state.

### pretext reuse: scroll = place-only

| Change | Invalidation class | v3a work | v3b work |
|--------|-------------------|----------|----------|
| scroll offset | `place-only` | none | reclip, reposition |
| filter text change | `measure+place` | redo text layout | remeasure + reclip |
| theme color only | `paint-only` | none | repaint |

v3b must NOT reshape text or remeasure boxes on scroll.

## Goal

1. Clipping + scroll: scissor clip via `Overflow::Hidden`, scroll damage math, scrollbar affordance
2. CPU effects: blur + drop shadow, budgets with deterministic degrade, cursor blink timer
3. IME/text-input stub: focus routing, caret/selection, keyboard → text input routing
4. **Filter-box proof element**: TextInput + `filter_words()` + scrollable filtered list
5. Host tests + OS markers + postflight

## Non-Goals

Kernel changes. Full IME engine (TASK-0146/0147). Keymaps/OSK. Clipboard daemon.
Text reshaping during scroll.

## Constraints

- Deterministic damage math and effect outputs
- Scroll = place-only: no text reshaping or layout remeasurement on scroll
- Bump-allocator safety: layout computation only in `new()`, scroll damage allocation-free
- No `unwrap/expect`

## Red flags

- **YELLOW (effects SSIM)**: prefer integer kernels; if pixel-exact impossible, use SSIM
- **YELLOW (IME gating)**: only focused surface receives text input; policy can deny IME

## Stop conditions

### Proof (Host)

`tests/ui_v3b_host/`:
- scroll damage rects match expected; clip respects boundaries
- `filter_words("ap")` returns `["apple","application","apt"]`
- filtered list height changes deterministically with result count
- scroll only invalidates list area, not input field
- blur/shadow goldens match; caret/selection renders correctly

### Proof (OS/QEMU)

Markers: `windowd: clipping on`, `windowd: scroll on`, `windowd: live scroll ok`,
`windowd: text input on`, `windowd: filter list ok`, `windowd: effects on`,
`windowd: effect blur ok`, `imed: ready`, `SELFTEST: ui v3 scroll ok`,
`SELFTEST: ui v3 ime ok`, `SELFTEST: ui v3 effect ok`, `SELFTEST: ui v3 filter ok`

### Visual proof

Filter-box visible on proof surface: keyboard input → visible text + cursor,
filtered list updates in real-time, scroll moves viewport with visible scrollbar,
clip boundaries visible on-screen.

## Touched paths

- `source/services/windowd/` + `idl/`
- `source/services/windowd/src/layout_panel.rs` (filter-box)
- `source/services/windowd/src/proof_panel_spec.rs` (FILTER_WORDS)
- `userspace/ui/effects/` (new)
- `userspace/ui/layout-types/src/node.rs` (TextInputNode)
- `source/services/imed/` (new)
- `tests/ui_v3b_host/` (new)
- `docs/dev/ui/foundations/layout/scroll.md`
- `docs/dev/ui/input/text-input.md`

## Plan

1. **Clipping + scroll**: IDL SetClip/SetScroll, damage math, scrollbar, markers ✅
2. **Text input + filter-box**: TextInputNode type, filter_words(), filter-box layout tree, keyboard routing, markers ✅
3. **Effects**: blur/shadow + cache + budgets, cursor blink timer, markers ✅
4. **IME/text input**: focus → text subscription, caret/selection helpers, imed stub ✅
5. **Proof + docs**: host tests + OS selftest + postflight ✅ (76 tests, OS markers defined)
6. **NeX UI Rendering Pipeline**: MSDF atlas (text+icons), SDF shapes (rounded rects, buttons), 9-slice shadow, dual-kawase blur, separable blur, render cache + damage integration, `BoxShadow`/`TextShadow`/`opacity` properties in `VisualStyle`, Tailwind shadow presets — see RFC-0058 Phase 6
7. **Compositor retained-mode upgrades** (2026-05-20):
   - Phase 1: TileMap wired into render loop (dirty-tile gating, clear-after-write)
   - Phase 2: LayerCache promoted from stub to functional (insert/get/invalidate)
   - Phase 3: Backdrop blur switched from inline to nexus_effects::blur_1d
   - Phase 4: Cursor-BG save/restore activated (dead fields → working)
   - Phase 5: Paint-only fast-path (skips non-paint boxes + backdrop blur)
   - Phase 6a: Shadow blur switched from inline blur_row_horizontal to nexus_effects::blur_1d (dead code removed)
   - Phase 6b/c: ShadowCache wiring (pending — needs offscreen shadow rendering)

## Touched paths (Phase 6)

- `userspace/ui/effects/src/{blur,shadow,budget,cache}.rs` (extend)
- `userspace/ui/layout-types/src/border.rs` (VisualStyle: BoxShadow, TextShadow, opacity)
- `source/services/windowd/src/os_lite.rs` (multi-pass renderer, shadow compositing)
- `userspace/ui/msdf/` (new: MSDF atlas generator + runtime sampler)
- `userspace/ui/sdf/` (new: analytical SDF shapes)
- `tests/ui_v4_host/` (new: shadow goldens, blur goldens, MSDF comparison)