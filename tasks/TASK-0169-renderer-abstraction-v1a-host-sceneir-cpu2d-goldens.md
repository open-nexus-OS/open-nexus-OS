---
title: TASK-0169 Renderer Abstraction v1a (host-first): Scene-IR + Backend trait + deterministic cpu2d + assets + golden snapshots
status: Draft
owner: @ui
created: 2025-12-26
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Driver/accelerator contracts (CPU now, GPU later): tasks/TRACK-DRIVERS-ACCELERATORS.md
  - UI v1a renderer baseline (to be refactored into this): tasks/TASK-0054-ui-v1a-cpu-renderer-host-snapshots.md
  - Text shaping baseline: tasks/TASK-0057-ui-v2b-text-shaping-svg-pipeline.md
  - Text stack integration contract: tasks/TASK-0148-textshape-v1-deterministic-bidi-breaks-shaping-contract.md
  - L10n/i18n + font fallback core: tasks/TASK-0174-l10n-i18n-v1a-host-core-fluent-icu-fontsel-goldens.md
  - Perf tracer (optional host hooks): tasks/TASK-0143-perf-v1a-perfd-frame-trace-metrics.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need a renderer stack that is:

- deterministic and host-testable (golden snapshots),
- backend-agnostic (CPU today; GPU later),
- aligned with `TRACK-DRIVERS-ACCELERATORS` (buffers/sync/budgets don’t drift into CPU-only assumptions).

Repo reality: current `windowd` is a placeholder and UI v1a renderer work is tracked as `TASK-0054`.
This task extracts a **clean renderer abstraction** and makes cpu2d the default backend.

OS wiring to `windowd` is handled in `TASK-0170`.

## Goal

Deliver:

1. Scene-IR core (`userspace/libs/scene-ir` or equivalent):
   - stable primitives: rect/rrect/path/layer/transform/paint/blend/image/text-run
   - strict `validate(scene)`:
     - reject NaNs/Infs
     - bounds caps (node count, image sizes, glyph counts)
     - deterministic error strings
   - markers (throttled):
     - `scene-ir: validate ok size=WxH`
     - `scene-ir: invalid <reason>`
2. Renderer abstraction (`userspace/libs/renderer`):
   - `Backend` trait: begin_frame/render/end_frame returning `PresentInfo`
   - cpu2d backend:
     - deterministic raster rules (AA off in v1)
     - deterministic blending (SrcOver/Multiply/Screen/Plus)
     - gradients via fixed 64-step LUT
     - images: PNG decode with explicit “ignore gamma/iCCP” rules (sRGB only)
     - text: consumes shaped runs from `userspace/ui/shape` / `userspace/textshape` bridge
     - bounded caches (glyph/image), deterministic LRU eviction
   - optional `wgpu` backend stub behind `renderer_wgpu_stub` feature:
     - methods return explicit `Unimplemented("wgpu stub")`
     - never enabled in CI/OS by default
   - markers (throttled):
     - `renderer: cpu2d frame WxH`
     - `renderer: cpu2d stats cpu_ms=<..> pixels=<..>`
3. Assets loader (`userspace/libs/assets`):
   - load-only from `pkg://` (tests use `pkg://fixtures/...`)
   - PNG + TTF subset (no hinting; deterministic)
   - caches with byte caps and deterministic eviction
   - markers (throttled):
     - `assets: font load id=<id>`
     - `assets: png decode WxH`
4. Golden snapshot host tests:
   - `tests/renderer_v1_host/` renders deterministic scenes and compares against repo-tracked goldens
   - update path only under `UPDATE_GOLDENS=1`
   - covers shapes/transforms/clips/blends/gradients/text/images

Font fallback note:

- cpu2d text rendering must support deterministic font fallback (Latin+CJK) via `fontsel` (`TASK-0174`) so mixed-script scenes can be rendered without missing glyphs.

## Non-Goals

- Kernel changes.
- Real OS present, vsync, or surface sharing (handled in `TASK-0170` / UI v1b).
- Analytic AA and subpixel text (v2+).
- Making `wgpu` a supported backend (stub only).
- A real wgpu backend implementation (tracked separately as `TASK-0171`, host-only).

## Constraints / invariants (hard requirements)

- Determinism first:
  - fixed pixel format pipeline (v1 chooses one canonical format; document it)
  - explicit rounding rules and integer math where feasible
  - no host filesystem ordering dependence in tests
- Bounded memory and node limits.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Alignment with `TRACK-DRIVERS-ACCELERATORS` (explicit)

- **Buffers**: renderer output is a “surface buffer” abstraction that can later be backed by VMO/filebuffer (no CPU-only assumptions).
- **Sync**: `PresentInfo` and future present fences must map cleanly to timeline fences (no ad-hoc events).
- **Budgets**: caches enforce hard caps and expose stats suitable for perf/power budgeting.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - Command(s):
    - `cargo test -p renderer_v1_host -- --nocapture`
  - Required:
    - golden snapshots pass deterministically
    - cache eviction and determinism tests pass
    - `renderer_wgpu_stub` feature compiles (optional) but is not in default features/CI

## Touched paths (allowlist)

- `userspace/libs/scene-ir/` (new)
- `userspace/libs/renderer/` (new)
- `userspace/libs/assets/` (new)
- `tests/renderer_v1_host/` (new)
- `docs/renderer/` (added in `TASK-0170` or here if minimal)

## Plan (small PRs)

1. Scene-IR types + validate + tests
2. Backend trait + cpu2d backend + deterministic rules + tests
3. Assets loader + caches + tests
4. Golden snapshot suite + docs

## Acceptance criteria (behavioral)

- Host goldens are byte-stable for the same fixtures.
- CPU backend is the default; wgpu is explicitly stubbed and disabled by default.
