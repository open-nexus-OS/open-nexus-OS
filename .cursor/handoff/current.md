# Handoff (compressed)

2026-06-11

## Last proven

- **TASK-0063** (UI v5b: Scene Graph GPU Pipeline + Virtual List + Theme Tokens + Virgl)
  - Phase 0: GPU pipeline hardening — scene graph drives rendering. 5 CPU modules deleted.
  - Phase 1: Scene graph 31 nodes (proof panel, cards, button, sidebar, chat, cursor). Animation wired.
  - Phase 2: ThemeRegistry with 2PC switching.
  - Phase 3: Virgl 3D protocol, context creation, GPU dispatch architecture.
  - Tests: 88 host tests passing (19 ui_v5b, 7 nexus-virtual-list, 62 windowd)
  - RFC-0063: Draft → updated

- **TASK-0062** (UI v5a): Done (2026-06-10), RFC-0059 Complete
- **TASK-0059** (UI v3b): Done (2026-06-05), RFC-0058 Complete

## Next steps

1. Virgl GPU shader: integrate TGSI/SPIR-V compiler for `submit_virgl_blur()` — currently returns Err, falls back to CPU
2. GPU text rendering: make `RenderPrimitive::Text` emit CB commands (glyph atlas + BlitSurface or DrawTiles)
3. QEMU `visible-bootstrap` test: verify all scene graph nodes render correctly
4. 120 Hz pacing proof: blocked on kernel timer capability (TASK-0062 Phase 7)

## Open risks

- Virgl GPU shader blocked on TGSI compiler — CPU separable gaussian is the current blur path
- GPU text is no-op — visual output has panels/cards/blur but no text labels
- Sidebar blur cache (Plane 3) not implemented in scene graph path

## Key architecture decisions

- Scene graph is single rendering authority — no CPU compositing fallback
- Blur dispatch: GPU-first → separable gaussian → box blur chain
- Animation: scene graph dirty set drives rendering, no damage rect queueing
