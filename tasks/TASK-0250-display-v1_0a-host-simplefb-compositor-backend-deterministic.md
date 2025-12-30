---
title: TASK-0250 Display v1.0a (host-first): simplefb compositor backend + premultiplied alpha + dirty rects + deterministic tests
status: Draft
owner: @ui
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Renderer abstraction baseline: tasks/TASK-0169-renderer-abstraction-v1a-host-sceneir-cpu2d-goldens.md
  - Windowd compositor baseline: tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need real framebuffer output for Display v1.0:

- simplefb compositor backend (ARGB8888),
- premultiplied alpha blending,
- dirty rect accumulation,
- deterministic host tests.

The prompt proposes a simplefb backend for windowd compositor. `TASK-0055` and `TASK-0170` already plan windowd compositor with VMO buffers and headless present. This task delivers the **host-first core** (simplefb backend, premultiplied alpha, dirty rects) that extends the renderer abstraction to support real framebuffer output.

## Goal

Deliver on host:

1. **Simplefb compositor backend** (`userspace/libs/renderer/backend_fb.rs`):
   - writes premultiplied-alpha ARGB8888 into a mapped buffer
   - dirty-rect accumulation per frame; union calculation
   - deterministic blending (premultiplied alpha pipeline)
   - sRGB assumption (HDR as TODO)
2. **Color operations library** (`userspace/libs/color/`):
   - premultiplied-alpha blend math
   - ARGB8888 format handling
   - deterministic color space conversions
3. **Dirty rect union**:
   - given N rects, compute union result deterministically
   - bounded accumulation (cap max rects per frame)
4. **Host tests** proving:
   - color ops: validate premultiplied-alpha blend math on small test tiles (golden hashes)
   - dirty union: given N rects, union result equals expected
   - vsync pace: simulated vsync produces deterministic sequences

## Non-Goals

- OS/QEMU framebuffer mapping (deferred to v1.0b).
- Real hardware (QEMU simplefb only).
- HDR support (sRGB only).

## Constraints / invariants (hard requirements)

- **No duplicate renderer backend**: This task extends the renderer abstraction from `TASK-0169` with a simplefb backend. Do not create a parallel rendering system.
- **Determinism**: premultiplied alpha blending, dirty rect union, and vsync pacing must be stable given the same inputs.
- **Bounded resources**: dirty rect accumulation is bounded; color operations are deterministic.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **YELLOW (premultiplied alpha vs straight alpha)**:
  - Premultiplied alpha is standard for compositing. Document the choice explicitly and ensure all blending uses premultiplied alpha consistently.

## Contract sources (single source of truth)

- Testing contract: `scripts/qemu-test.sh`
- Renderer abstraction: `TASK-0169` (Scene-IR + Backend trait)
- Windowd compositor: `TASK-0055` (surfaces/layers IPC + vsync)

## Stop conditions (Definition of Done)

### Proof (Host) — required

`cargo test -p display_simplefb_v1_0_host` green (new):

- color ops: validate premultiplied-alpha blend math on small test tiles (golden hashes)
- dirty union: given N rects, union result equals expected
- vsync pace: simulated vsync produces deterministic sequences

## Touched paths (allowlist)

- `userspace/libs/renderer/backend_fb.rs` (new; simplefb backend)
- `userspace/libs/color/` (new; premultiplied alpha)
- `tests/display_simplefb_v1_0_host/` (new)
- `docs/display/simplefb_v1_0.md` (new, host-first sections)

## Plan (small PRs)

1. **Color operations + premultiplied alpha**
   - premultiplied-alpha blend math
   - ARGB8888 format handling
   - host tests

2. **Dirty rect union**
   - union calculation
   - bounded accumulation
   - host tests

3. **Simplefb backend stub**
   - backend trait implementation
   - host tests (with fake buffer)

4. **Docs**
   - host-first docs

## Acceptance criteria (behavioral)

- Premultiplied-alpha blend math works correctly.
- Dirty rect union calculation is correct.
- Simplefb backend integrates with renderer abstraction.
