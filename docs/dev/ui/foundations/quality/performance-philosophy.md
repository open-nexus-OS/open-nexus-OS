<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# UI Performance Philosophy

This page defines the default performance stance for the Open Nexus OS UI stack.

The goal is not "fancy UI at any cost". The goal is:

- rich visuals where they add value,
- low and predictable overhead,
- bounded degradation under pressure,
- and an architecture that scales from QEMU bring-up to real hardware without needing a rewrite.

## Core stance

UI effects, animation, and composition must be treated as a **system performance contract**, not as purely visual polish.

In practice this means:

- `windowd` and the renderer are valid performance test surfaces for the whole system,
- bulk bytes travel on the VMO/filebuffer data plane, not as oversized control-plane payloads,
- expensive visuals must be damage-aware, cached, and degradable,
- idle UI should be cheap.

## Hot-path budget vocabulary

When evaluating architecture changes, prefer explicit budgets over vague “fast enough” claims.

Useful budgets include:

- service hops per user action,
- cross-core hops per user action,
- queue transitions per interaction,
- queue residence time by QoS class,
- recompute fanout per state mutation,
- observer count per commit,
- useful vs wasted recompute ratio,
- wakeups per interaction,
- global synchronization points per frame,
- and copy-fallback / mapping-reuse counters for bulk payload paths.

Not every budget needs an immediate hard threshold, but every hot path should eventually be explainable in these terms.

## Rules

### 1. No full-scene work by default

- No full-screen blur, repaint, or recomposition unless it is explicitly justified.
- Default assumption: only changed regions should trigger meaningful work.

### 2. Damage stays authoritative

- Damage/dirty rects drive rendering, backdrop refresh, and present decisions.
- If there is no damage and no visible state change, skip compose/present deterministically.

### 3. Glass is per-surface, not global

- Glass/backdrop effects apply to bounded surfaces such as sidebars, sheets, popovers, and control surfaces.
- Prefer cached backdrop snapshots per surface.
- Do not treat blur as a whole-scene post-process.

### 4. Frozen glass when idle

- If the glass surface and the background behind it are unchanged, keep the cached backdrop.
- Foreground UI may continue animating without forcing live backdrop refresh.

### 5. Animation has a frame budget

- Every animation path must have bounded per-frame work.
- Property changes in the same frame should coalesce where possible.
- Over-budget behavior must degrade deterministically instead of causing runaway jank.

### 6. Effects must degrade explicitly

- Under pressure, reduce quality in a documented order:
  - fewer refreshes,
  - higher downsample,
  - cached/frozen backdrop,
  - opaque fallback where needed.
- No hidden "try harder and hope" paths.

### 7. Bulk never rides the control plane

- Large surfaces, screenshots, images, media frames, and similar bulk payloads should travel via VMO/filebuffer.
- Typed IPC messages stay small and control-oriented.

### 8. Idle mode should be close to free

- An idle desktop should not burn cycles on invisible presents, live blur refresh, or busy animation timelines.
- The system should naturally settle into a low-work state when nothing visible is changing.

### 9. Accessibility and low-power fallbacks are first-class

- Reduced motion and reduce transparency are not secondary features.
- They are part of the default performance strategy and must share the same layout/interaction semantics.

### 10. Determinism beats cleverness

- Prefer deterministic region math, blur kernels, pacing rules, and degrade paths.
- Avoid renderer-dependent tricks that make host goldens or QEMU behavior flaky.

### 11. Optimize the common case first

- Cheap paths for unchanged buffers, small control messages, reused mappings, and common focus/hit-test flows matter more than exotic peak paths.
- Fix avoidable copies and wakeups before adding scheduler or compositor complexity.

### 12. QEMU fluidity is a gate, not the final truth

- If glass, transitions, and layered overlays already behave fluidly in QEMU, that is a strong sign the architecture is healthy.
- But QEMU is not the final device model; keep measuring again on real hardware once available.

## What success looks like

Healthy UI performance usually looks like this:

- idle UI is quiet,
- hover/focus/click changes touch only small regions,
- glass surfaces refresh only when they need to,
- animations remain smooth or degrade gracefully,
- and expensive visuals never force a whole-system performance collapse.

## Related

- `docs/dev/ui/foundations/visual/materials.md`
- `docs/dev/ui/foundations/rendering/compositor.md`
- `docs/dev/ui/foundations/visual/effects.md`
- `docs/dev/ui/foundations/animation.md`
- `docs/dev/ui/foundations/quality/testing.md`
