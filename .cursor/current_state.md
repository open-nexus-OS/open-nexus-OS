# Current State (compressed)

2026-06-12 — TASK-0063 Done. GPU pipeline + scene graph + virtual list + theme tokens + virgl architecture complete.

## Last completed

- **TASK-0063** (UI v5b: Scene Graph GPU Pipeline + Virtual List + Theme Tokens + Virgl) — **Done** (2026-06-12)
  - Phase 0: GPU pipeline hardening — scene graph is sole rendering authority. CPU compositor deleted (backdrop, scene, shadow, surface, source).
  - Phase 1: Scene graph extended (MAX_NODES=2048, batch_insert, recycling, Group shadow). SystemUiShell rebuilt with 31 nodes (proof panel, 4 cards, glass button, sidebar, chat panel, cursor). Animation wired through scene graph.
  - Phase 2: Theme tokens — ThemeRegistry with 2PC-ready switching.
  - Phase 3: Virgl architecture — 3D protocol (CTX_CREATE, SUBMIT_3D), context creation, separable gaussian blur. GPU shader dispatch (submit_virgl_blur) is architectural target; currently falls back to CPU separable gaussian.
  - Host tests: ui_v5b_host (19 tests), nexus-virtual-list (7 tests), windowd (62 tests) — 88 tests, 0 failures.
  - Build: windowd + gpud + nexus-virtual-list + nexus-theme + ui_v5b_host clean.
  - RFC-0063: Complete.

- **TASK-0062** (UI v5a: Animation + NexusGfx) — Done (2026-06-10)
  - RFC-0059: Complete

- **TASK-0059** (UI v3b: clip/scroll/effects) — Done (2026-06-05)
  - RFC-0058: Complete

## Known risks / DON'T DO

- DON'T add debug logs in kernel
- DON'T fake-success markers for stub paths
- Virgl GPU shader dispatch blocked on TGSI/SPIR-V compiler integration (submit_virgl_blur returns Err → falls back to CPU)
- GPU text rendering is no-op (Text primitive returns Ok(0)) — follow-up: TASK-0275 or TASK-0064
- OS-build blockers documented in RFC-0063 delta analysis; CPU compositor modules restored, not yet deleted

## Open threads

- TASK-0062 Phase 7: kernel timer capability package (6-8d)
- TASK-0063 Phase 3 virgl GPU shader: TGSI compiler integration (deferred to follow-up)
- TASK-0063 GPU text rendering: Text primitive → CB commands (deferred to follow-up)
- Security group progress: 3/36 (8%)
- Kernel production-grade closure blockers: TASK-0286..0290

## Architecture drift

- No drift. GPU-first pipeline (scene graph → CommandBuffer → gpud) is locked per RFC-0063 (Complete).
- Scene graph is single rendering authority. No CPU compositing.
- Dual-path CPU+GPU removed; single owner, single path.
