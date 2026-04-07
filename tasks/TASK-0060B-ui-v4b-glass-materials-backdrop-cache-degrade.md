---
title: TASK-0060B UI v4b: glass materials + backdrop snapshots + cached blur + deterministic degrade
status: Draft
owner: @ui
created: 2026-03-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Glass material guidance: docs/dev/ui/foundations/visual/materials.md
  - UI v3b effects baseline: tasks/TASK-0059-ui-v3b-clip-scroll-effects-ime-textinput.md
  - UI v4a compositor perf baseline: tasks/TASK-0060-ui-v4a-tiled-compositor-clipstack-atlases-perf.md
  - UI v5a animation baseline: tasks/TASK-0062-ui-v5a-reactive-runtime-animation-transitions.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Open Nexus OS already documents a resource-conscious “glass” material recipe in `docs/dev/ui/foundations/visual/materials.md`:

- backdrop snapshot,
- downsample + separable blur,
- tint + edge highlight,
- cached / frozen glass when idle,
- degrade and reduce-transparency fallback under pressure.

What is missing is an execution task that turns that guidance into a bounded compositor feature. This task exists
because glass, translucency, and layered overlays are both:

- an important UX goal,
- and a very good system-wide performance test for damage math, caching, pacing, and hot-path discipline.

## Goal

Deliver a bounded, QEMU-tolerant glass material implementation:

1. **Glass materials in windowd**:
   - support `material.glassLow` and `material.glassHigh`,
   - token-driven, deterministic rendering.
2. **Backdrop snapshot path**:
   - capture scene content behind the glass surface,
   - downsample + blur via the existing CPU effect stack.
3. **Cached / frozen glass**:
   - cache backdrop state per glass surface,
   - refresh only when the surface or intersecting background changes.
4. **Deterministic degrade policy**:
   - throttle live backdrop refresh under pressure,
   - reduce blur quality or fall back to opaque material deterministically,
   - respect reduce-transparency / low-power policy.

## Non-Goals

- Physically correct refraction or distortion.
- GPU-only effects or renderer-specific tricks.
- Unbounded live blur on the whole scene.
- Replacing the existing theme/material token system.

## Constraints / invariants (hard requirements)

- Rendering must stay deterministic enough for host goldens.
- Glass is applied per surface/overlay, not per widget subtree.
- Refresh rules must be damage-aware and bounded.
- Fallbacks must be explicit:
  - cached/frozen glass,
  - lower-quality blur,
  - opaque surface mode.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v4b_glass_host/`:

- static background → cached glass matches expected output,
- background damage intersecting the glass region triggers a refresh,
- animating glass surface uses bounded refresh updates,
- reduce-transparency switches to deterministic opaque fallback,
- degrade path changes quality mode deterministically under injected budget pressure.

### Proof (OS/QEMU) — gated

UART markers (order tolerant):

- `windowd: glass on`
- `windowd: glass cached`
- `windowd: glass refresh ok`
- `windowd: glass degrade -> cached|low|opaque`
- `SELFTEST: ui glass ok`

## Touched paths (allowlist)

- `source/services/windowd/`
- `userspace/ui/effects/`
- `userspace/ui/renderer/`
- `tests/ui_v4b_glass_host/` (new)
- `source/apps/selftest-client/`
- `docs/dev/ui/foundations/visual/materials.md`
- `docs/dev/ui/foundations/rendering/compositor.md`
- `docs/dev/ui/foundations/quality/testing.md`

## Plan (small PRs)

1. connect material tokens to a backdrop-snapshot compositor path
2. add cached/frozen glass refresh rules and counters
3. add degrade / reduce-transparency behavior
4. add host/QEMU scenes that keep glass as a first-class perf gate
