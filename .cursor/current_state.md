# Current State (compressed)

2026-06-12 — TASK-0063 Done. TASK-0064 rescoped: Chat-Window als erste WM-Implementierung.

## Last completed

- **TASK-0063** (UI v5b: Scene Graph GPU Pipeline + Virtual List + Theme Tokens + Virgl) — **Done** (2026-06-12)
  - Phase 0: GPU pipeline hardening — scene graph is sole rendering authority.
  - Phase 1: Scene graph 31 nodes, animation wired.
  - Phase 2: ThemeRegistry with 2PC switching.
  - Phase 3: Virgl architecture complete.
  - Tests: 88 host tests passing. RFC-0063: Complete.

- **TASK-0062** (UI v5a: Animation + NexusGfx) — Done (2026-06-10), RFC-0059 Complete
- **TASK-0059** (UI v3b: clip/scroll/effects) — Done (2026-06-05), RFC-0058 Complete

## Active

- **TASK-0064** (UI v6a: Window Management v1 — Chat-Window mit Drag, Title-Bar, Z-Order) — **Draft**
  - RFC-0064: Draft (design seed)
  - Scope: Chat wird dragbares, schließbares Window. Chat-Button neben Hamburger. Title-Bar + X.
  - Non-Goals: kein Multi-Window, kein Resize, kein IPC, keine Transitions (→ TASK-0064B)
  - Depends on: TASK-0063 (scene graph)

## Known risks / DON'T DO

- DON'T add debug logs in kernel
- DON'T fake-success markers for stub paths
- Virgl GPU shader dispatch blocked on TGSI/SPIR-V compiler
- GPU text rendering is no-op — deferred to follow-up

## Open threads

- TASK-0062 Phase 7: kernel timer capability package (6-8d)
- TASK-0063 Phase 3 virgl GPU shader: TGSI compiler integration (deferred)
- TASK-0063 GPU text rendering: Text primitive → CB commands (deferred)
- TASK-0064B: Scene Transitions (Crossfade/Slide) — deferred
- Security group progress: 3/36 (8%)

## Architecture drift

- No drift. GPU-first pipeline (scene graph → CommandBuffer → gpud) locked per RFC-0063 (Complete).
- Window management: Chat-Window als Konkretisierung statt abstraktem WM-Layer. RFC-0064 (Draft).
