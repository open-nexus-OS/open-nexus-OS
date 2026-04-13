---
title: TASK-0171 Renderer v1 (host-only): wgpu backend (feature-gated) + offscreen targets + parity vs cpu2d (SSIM/tolerances) + tools/docs
status: Draft
owner: @ui
created: 2025-12-26
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Driver/accelerator contracts (GPU later): tasks/TRACK-DRIVERS-ACCELERATORS.md
  - Renderer Abstraction v1a (Scene-IR + cpu2d): tasks/TASK-0169-renderer-abstraction-v1a-host-sceneir-cpu2d-goldens.md
  - Renderer Abstraction v1b (windowd wiring): tasks/TASK-0170-renderer-abstraction-v1b-os-windowd-wiring-textshape-perf-markers.md
  - Text shaping baseline: tasks/TASK-0057-ui-v2b-text-shaping-svg-pipeline.md
  - Perf tracer (optional): tasks/TASK-0143-perf-v1a-perfd-frame-trace-metrics.md
---

## Context

Renderer v1 defaults to a deterministic CPU backend (`cpu2d`) with goldens (`TASK-0169`).
We also want to validate the backend abstraction against a GPU-style backend without changing OS/CI:

- implement a real `wgpu` backend behind a **feature flag** (disabled by default),
- render offscreen (headless) into an RGBA8 target and read back,
- compare outputs against `cpu2d` using deterministic tolerance metrics (SSIM + max abs error),
- keep OS/QEMU and CI builds on cpu2d.

This task is **host-only**: it must not introduce a required GPU dependency for OS/CI.

## Goal

Deliver:

1. `wgpu` backend implementation of `renderer::Backend`:
   - offscreen target: RGBA8 sRGB texture sized to `Frame`
   - `end_frame()` performs readback to CPU memory (required for tests and tools)
   - deterministic IR translation:
     - rect/rrect/path tessellation into triangles (deterministic ordering)
     - gradients via fixed 64-step LUT (match cpu2d)
     - images sampled with nearest filter (match cpu2d v1)
     - text via glyph atlas (single-channel alpha) and a deterministic upload schedule
     - blend modes: SrcOver/Multiply/Screen/Plus implemented explicitly
2. Feature gating:
   - `renderer_wgpu` feature is **off by default**
   - CI must not enable it by default
   - if enabled, it must compile and tests must run
3. Runtime selection (host-only):
   - `nx-render` supports `--backend cpu2d|wgpu`
   - if `wgpu` requested without feature, fail clearly (no silent fallback)
4. Parity test suite (host-only; compiled only with `--features renderer_wgpu`):
   - render identical scenes with cpu2d and wgpu
   - compare using:
     - SSIM (deterministic implementation; fixed window)
     - max per-channel absolute error
   - default thresholds:
     - `SSIM ≥ 0.995`
     - `max_abs_err ≤ 3` (0–255)
   - allow per-case overrides with explicit justification (e.g., text edge rasterization)
5. Docs:
   - how to enable the backend and run parity tests
   - determinism policy and known gaps (AA remains off, nearest sampling, tess edge cases)

## Non-Goals

- Kernel changes.
- Shipping wgpu backend in OS/QEMU builds.
- Making wgpu the default renderer backend.
- Adding analytic AA or high-quality sampling (v2+).

## Constraints / invariants (hard requirements)

- Determinism:
  - parity metrics must not use nondeterministic floating reductions
  - adapter selection must be conservative (headless; lowest common denominator)
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake success:
  - any missing feature must be explicit (error), never a silent cpu2d fallback for `--backend wgpu`.

## Alignment with `TRACK-DRIVERS-ACCELERATORS`

- This backend is a **validation** of the “UI backend abstraction” contract (CAND-UI-000).
- It must not introduce new buffer/sync semantics that would conflict with a future GPU driver service model.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - Command(s):
    - `cargo test -p renderer_wgpu_host --features renderer_wgpu -- --nocapture`
  - Required proofs:
    - parity suite passes within thresholds on supported host platforms
    - `nx-render --backend wgpu` produces a PNG for a fixture scene
    - default build remains cpu2d-only (no wgpu linked)

## Touched paths (allowlist)

- `userspace/libs/renderer/` (add wgpu backend module/crate behind feature)
- `tools/nx-render/` (backend selection)
- `tests/renderer_wgpu_host/` (new; feature-gated)
- `docs/renderer/wgpu.md` + `docs/renderer/testing.md`
- `justfile` (dev-only target to run host wgpu)

## Plan (small PRs)

1. Implement wgpu backend skeleton + feature gating (no default enable)
2. Implement IR translation subset to reach parity for v1 scenes
3. Add parity metrics (SSIM + max err) and feature-gated tests
4. Tooling + docs + dev-only targets

## Acceptance criteria (behavioral)

- With `--features renderer_wgpu`, host parity tests pass; without it, the workspace remains unchanged and cpu2d stays default.
