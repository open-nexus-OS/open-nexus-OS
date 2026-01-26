---
title: TASK-0170 Renderer Abstraction v1b (OS/QEMU): windowd/compositor wiring to Backend + textshape bridge + present markers (CPU2D default)
status: Draft
owner: @ui
created: 2025-12-26
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Driver/accelerator contracts (CPU now, GPU later): tasks/TRACK-DRIVERS-ACCELERATORS.md
  - Renderer abstraction host slice: tasks/TASK-0169-renderer-abstraction-v1a-host-sceneir-cpu2d-goldens.md
  - UI v1b windowd baseline: tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md
  - Text shaping baseline: tasks/TASK-0057-ui-v2b-text-shaping-svg-pipeline.md
  - Perf hooks (optional): tasks/TASK-0144-perf-v1b-instrumentation-hud-nx-perf.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

`TASK-0169` delivers a host-first Scene-IR + renderer backend abstraction with a deterministic cpu2d backend.
This task wires OS `windowd` (compositor) to:

- build a root Scene per frame,
- render via the backend trait,
- emit deterministic present markers in QEMU.

This is aligned with `TRACK-DRIVERS-ACCELERATORS` so a future GPU service can slot in behind the same contracts.

## Goal

Deliver:

1. `windowd` wiring:
   - on vsync tick:
     - `backend.begin_frame(frame)`
     - build Scene-IR root from surfaces/layers (v1: minimal transforms/opacity/clips)
     - `backend.render(scene)`
     - `backend.end_frame()` and emit present markers
   - markers:
     - `windowd: compose begin`
     - `windowd: compose end`
     - `windowd: present ok`
2. Text bridge wiring:
   - `windowd` uses `textshape` outputs to build `TextRun` nodes in Scene-IR
   - default path remains deterministic (hb-off if OS path cannot support HarfBuzz; see `TASK-0057` red flag)
3. Perf hook (optional, gated):
   - if `perfd` exists, send a per-frame tick with cpu_ms/pixels stats
   - must not emit “perf ok” markers unless the gate is real (`TASK-0145` later)
4. OS selftests (bounded):
   - compose smoke scene, wait for `windowd: present ok`
   - markers:
     - `SELFTEST: renderer v1 present ok`
     - `SELFTEST: renderer v1 perf ok` (only if perfd + hooks exist; otherwise explicit `stub/placeholder`)

## Non-Goals

- Kernel changes.
- Real display output and GPU acceleration.
- Full present scheduler / fences semantics beyond v1 (tracked in UI v2+ tasks).
- Simplefb framebuffer backend (handled by `TASK-0250`/`TASK-0251` as an extension; this task focuses on headless present with VMO buffers).

## Constraints / invariants (hard requirements)

- Deterministic markers and bounded timeouts (no busy-wait).
- CPU2D is default; wgpu backend remains disabled.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake success:
  - perf marker must be skipped or explicit placeholder until perf stack exists

## Red flags / decision points (track explicitly)

- **RED (windowd is currently placeholder code)**:
  - real compositor wiring is a larger body of work. This task should either:
    - replace the placeholder windowd with a minimal real compositor loop, or
    - explicitly gate on `TASK-0055` if windowd compositor infrastructure is not yet present.

## Stop conditions (Definition of Done)

- **Proof (QEMU)**:
  - Command(s):
    - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=185s ./scripts/qemu-test.sh`
  - Required markers:
    - `windowd: present ok`
    - `SELFTEST: renderer v1 present ok`
    - `SELFTEST: renderer v1 perf ok` (only if perfd is present; otherwise explicit placeholder)

## Touched paths (allowlist)

- `source/services/windowd/` (replace placeholder with real wiring)
- `source/apps/selftest-client/`
- `scripts/qemu-test.sh`
- `docs/renderer/overview.md` + `docs/renderer/testing.md` + `docs/dev/ui/architecture.md`

## Plan (small PRs)

1. Implement minimal windowd render loop using `renderer::Backend`
2. Wire textshape bridge and minimal scene building (clip/transform/opacity)
3. Add selftest markers + docs

## Acceptance criteria (behavioral)

- In QEMU, windowd renders via the Backend trait and emits present markers deterministically.
