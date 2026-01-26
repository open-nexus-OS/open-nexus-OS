---
title: TASK-0057 UI v2b: text shaping (HarfBuzz) + font fallback/cache + SVG safe subset pipeline + headless tests
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - UI v1a renderer (baseline): tasks/TASK-0054-ui-v1a-cpu-renderer-host-snapshots.md
  - UI v2a (present/input baseline): tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md
  - Drivers/Accelerators contracts (future GPU backend): tasks/TRACK-DRIVERS-ACCELERATORS.md
  - Policy as Code (asset access): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
---

## Context

For real UI, we need:

- correct shaping for complex scripts (bi-di, ligatures),
- a minimal vector pipeline for icons/theming,
- and deterministic tests that can run on host.

This task focuses on the content pipeline and rendering primitives, independent of present scheduling (v2a).

Scope note:

- L10n/i18n locale switching and font fallback selection are tracked separately as `TASK-0174`/`TASK-0175`.
  Shaping here should consume a deterministic font fallback chain rather than inventing its own locale plumbing.

We keep the renderer backend-agnostic:

- CPU rasterization is the initial backend.
- A future GPU backend can reuse the same shaping outputs and SVG tessellation outputs.

## Goal

Deliver:

1. `userspace/ui/shape`:
   - HarfBuzz-based shaping
   - script-aware fallback chain (small and explicit)
   - glyph run output suitable for a raster backend
2. Renderer glyph cache:
   - grayscale alpha glyph bitmaps
   - bounded cache sizes and eviction
3. `userspace/ui/svg` safe subset:
   - parse a strict subset and reject the rest
   - rasterize into BGRA8888 (CPU) deterministically
4. Host tests:
   - shaping goldens (JSON)
   - SVG render goldens (PNG with SSIM threshold if needed)

## Non-Goals

- Kernel changes.
- Full SVG spec.
- LCD subpixel text.

## Constraints / invariants (hard requirements)

- Deterministic outputs:
  - stable shaping inputs/outputs (with explicit tolerances where unavoidable),
  - stable SVG rasterization rules.
- Strict safety posture:
  - SVG subset parser must reject unsupported features (no scripts, no external refs, no filters).
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Bounded memory:
  - cap glyph cache bytes,
  - cap SVG complexity (nodes/path segments).

## Red flags / decision points

- **YELLOW (HarfBuzz in OS)**:
  - If the OS build environment cannot support HarfBuzz (std/alloc constraints), we may need:
    - host-precomputed shaped runs (asset pipeline), or
    - a smaller shaping approach for OS.
  - v2b should start host-first and only enable OS path once feasible.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v2b_host/`:

- shaping:
  - multilingual strings (LTR/RTL) produce stable glyph cluster ordering
  - advance widths within tolerance of goldens
- glyph cache:
  - repeated draws hit cache deterministically
  - eviction occurs at configured cap
- SVG:
  - safe subset parses accepted files
  - rejected features are correctly detected
  - rendered PNGs match goldens (pixel-exact or SSIM threshold)

## Touched paths (allowlist)

- `userspace/ui/shape/` (new)
- `userspace/ui/svg/` (new)
- `userspace/ui/renderer/` (extend: draw_glyph_run + glyph cache)
- `tests/ui_v2b_host/` (new)
- `docs/dev/ui/text.md` + `docs/dev/ui/svg.md` (new)

## Plan (small PRs)

1. **Shaping crate**
   - FontManager with explicit fallback set
   - `shape_text()` → `GlyphRun`s

2. **Renderer integration**
   - glyph cache + `draw_glyph_run`

3. **SVG safe subset**
   - strict parser + deterministic rasterizer

4. **Tests + docs**
   - goldens + tolerance policy documented
