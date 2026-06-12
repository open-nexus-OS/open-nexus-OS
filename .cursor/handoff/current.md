# Handoff (compressed)

2026-06-12

## Last completed

- **TASK-0063** (UI v5b: Scene Graph GPU Pipeline + Virtual List + Theme Tokens + Virgl) — **Done**
  - Tests: 88 host tests passing. RFC-0063: Complete.

- **TASK-0062** (UI v5a): Done (2026-06-10), RFC-0059 Complete
- **TASK-0059** (UI v3b): Done (2026-06-05), RFC-0058 Complete

## Active

- **TASK-0064** (UI v6a: Window Management v1 — Chat-Window + Drag) — **Draft** (rescoped 2026-06-12)
  - RFC-0064: Draft design seed
  - Chat wird dragbares Window mit Title-Bar, X-Close, Z-Order
  - Chat-Button links neben Hamburger-Menu
  - Non-Goals: kein Multi-Window, kein Resize, keine Transitions
  - Follow-up: TASK-0064B (Scene Transitions)

## Next steps

1. TASK-0064 Phase 0: Chat-Button + Window/WindowManager structs
2. TASK-0064 Phase 1: Title-Bar + X-Button + Drag
3. TASK-0064 Phase 2: Integration + Host-Tests + QEMU-Marker

## Open risks

- Virgl GPU shader blocked on TGSI compiler — CPU fallback
- GPU text is no-op — text labels not visible
- OS build: CPU compositor modules restored but not deleted

## Key architecture decisions

- Scene graph is single rendering authority — no CPU compositing fallback
- Window management starts concrete (Chat-Window) rather than abstract (WM layer)
- Z-Order: Chat-Window > Sidebar > Proof-Panel > Wallpaper
- Title-Bar only as drag handle (standard OS pattern)
- Scene transitions (crossfade/slide) deferred to TASK-0064B
