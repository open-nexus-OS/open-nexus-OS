---
title: TASK-0059 UI v3b: clipping/scroll layers + precise damage + CPU effects (blur/shadow) + IME/text-input stub
status: Draft
owner: @ui
created: 2025-12-23
updated: 2026-05-15 (RFC-0057 dependency clarified — v3b consumes v3a layout tree)
depends-on: [TASK-0058]
follow-up-tasks: []
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - **RFC seed (layout contract)**: docs/rfcs/RFC-0057-ui-v3a-layout-engine-pretext-contract.md
  - UI v3a layout/wrap: tasks/TASK-0058-ui-v3a-layout-wrapping-deterministic.md
  - UI v2a present/input baseline: tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md
  - UI v2a embedded runtime/reactor floor: tasks/TASK-0056C-ui-v2a-present-input-perf-latency-coalescing.md
  - UI v2b shaping/svg baseline: tasks/TASK-0057-ui-v2b-text-shaping-svg-pipeline.md
  - UI v1b windowd baseline: tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md
  - Layout pipeline contract: docs/dev/ui/foundations/layout/layout-pipeline.md
  - Scroll spec: docs/dev/ui/foundations/layout/scroll.md
  - Glass material guidance: docs/dev/ui/foundations/visual/materials.md
  - Glass compositor follow-up: tasks/TASK-0060B-ui-v4b-glass-materials-backdrop-cache-degrade.md
  - Drivers/Accelerators contracts: tasks/TRACK-DRIVERS-ACCELERATORS.md
  - Config broker (budgets): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Policy as Code (IME focus guards): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

With layout/wrapping available (v3a), v3b adds:

- correct clipping + scroll layers with precise damage math,
- live pointer-driven scroll and scrollbar/hover feedback for QEMU-visible UI,
- CPU composition effects with budgets (blur/shadow),
- a minimal IME/text-input stub path (composition/commit, caret/selection).

This task is QEMU-tolerant but has more moving parts, so it is gated on prior UI v1/v2 tasks.
It extends the embedded reactor/runtime floor established by `TASK-0056C`, rather than
introducing a second present/runtime path for scrolling and visible effects.

Sequencing note:

- v3b owns the **effect primitives** (blur/shadow + budgets).
- Explicit backdrop-driven "glass" materials are a later compositor consumer of those primitives and are tracked in
  `TASK-0060B`.

### pretext reuse: why v3b depends on v3a

The layout engine from TASK-0058 follows the **pretext philosophy**: paragraph/run preparation and line-layout
are cached separately (width-independent vs width-dependent). This enables v3b's scroll to be **cheap**:

| Change | Invalidation class | v3a work | v3b work |
|--------|-------------------|----------|----------|
| **scroll offset** | `place-only` | none — layout boxes reused | reclip, reposition |
| width bucket change | `measure+place` | redo line layout | remeasure + reclip |
| text content change | `text-prep+measure+place` | reshape + relayout | full subtree |
| theme color/token only | `paint-only` | none | repaint |

Scroll is the most frequent user interaction. Because v3a caches paragraph/run data separately from line-layout,
scrolling only changes **placement** — all layout boxes, text advances, and line breaks stay valid.
v3b's scroll damage math operates on the **stable layout tree** from v3a, computing dirty rects as the
difference between old and new viewport positions within unchanged layout boxes.

This means v3b must NOT:
- reshape text on scroll,
- remeasure layout boxes on scroll,
- or rebuild the layout tree on scroll.

It must only reclip, reposition, and repaint.

## Goal

Deliver:

1. `windowd` clipping + scroll layers:
   - scissor clipping using v3a layout boxes as clip rects (containers with `Overflow::Hidden` set the clip boundary)
   - scroll offsets and scroll damage rules (viewport delta → dirty rect set)
   - live QEMU pointer wheel/drag scroll routed through the `TASK-0253` input path and `TASK-0056B` visible affordance semantics
   - visible scroll affordance hover/active state for proof surfaces
   - preserve the `TASK-0056C` idle-cheap/no-damage fast path whenever scroll state is unchanged
   - use a small visible scroll/clip window on the shared proof surface rather than a detached demo
   - scroll must NOT trigger text reshaping or layout remeasurement (pretext place-only contract)
2. CPU effects module:
   - separable blur and drop shadow
   - caching and per-frame budgets with deterministic degrade behavior
3. Minimal IME/text-input:
   - IME/text-input protocol plumbing (focus routing + caret/selection integration)
   - `imed` is introduced as a stub only if IME v2 tasks are not yet landed
   - text-input protocol for focused surface
   - caret/selection rendering helpers (using v3a text layout metrics for positioning)
4. Host tests (damage/effects/IME flow) and OS markers + postflight.

## Non-Goals

- Kernel changes.
- Full IME engine (language models, dictionaries). v3b is protocol plumbing only.
- US/DE keymaps, dead keys/compose tables, OSK overlay, and IME host behavior (tracked as IME/Text v2 Part 1: `TASK-0146`/`TASK-0147`).
- Clipboard daemon creation if it doesn't exist yet (integration is optional and gated).
- Text reshaping or layout remeasurement during scroll (pretext place-only contract).

## Constraints / invariants (hard requirements)

- Deterministic damage math and deterministic effect outputs.
- Live scroll must be routed through `windowd`; selftest scroll injection is regression coverage, not a substitute for QEMU pointer proof.
- Scroll/clip/effect invalidation must extend the `TASK-0056C` runtime/reactor floor instead of bypassing it with unconditional full-frame present work.
- **Scroll = place-only**: scroll offsets must not cause text reshaping, line re-breaking, or layout box remeasurement. Only clipping and positioning change.
- Strict budgets:
  - cap blur radius and area per frame,
  - cap cached effect entries and total bytes.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Clear "what is stubbed" for IME and clipboard integration.

## Red flags / decision points

- **YELLOW (effects determinism vs SSIM)**:
  - CPU blur/shadow should be deterministic, but tiny platform differences may appear.
  - Prefer integer kernels and explicit rounding to keep pixel-exact possible; if not, use SSIM with a documented threshold.
- **YELLOW (IME gating)**:
  - IME focus must be tightly scoped: only the focused surface receives text input events.
  - Policy should be able to deny IME use for non-UI subjects (documented).

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v3b_host/`:

- scroll damage:
  - given a scroll delta, computed damage rect set matches expected (order-agnostic)
  - scroll damage math operates on v3a layout boxes without remeasuring them
  - live pointer wheel/drag scroll updates visible content and scrollbar/hover state in QEMU proof
- clipping:
  - clipped layers do not paint outside clip; damage respects clip
  - clip rects derived from v3a layout box coordinates
- effects:
  - blur/shadow output matches goldens (pixel-exact or SSIM threshold)
  - budget trip path degrades deterministically and records a marker/counter
- IME:
  - mocked IME compose/commit updates a model and renders caret/selection correctly
  - caret position derived from v3a text layout metrics

### Proof (OS/QEMU) — gated

UART markers (order tolerant):

- `windowd: clipping on`
- `windowd: scroll on`
- `windowd: live scroll ok`
- `windowd: effects on`
- `windowd: effect blur ok`
- `imed: ready`
- `SELFTEST: ui v3 scroll ok`
- `SELFTEST: ui v3 ime ok`
- `SELFTEST: ui v3 effect ok`
- `SELFTEST: ui v3 wrap ok` (carried from v3a; wrapping proven by v3a, reaffirmed by v3b)

### Visual proof — required

- the shared proof surface contains a small scrollable window/panel,
- live wheel/drag visibly moves content inside that panel and updates the scroll affordance,
- clip boundaries are visible on-screen and not only asserted in headless math tests,
- scrolling does not cause visible text re-layout or re-wrapping.

## Touched paths (allowlist)

- `source/services/windowd/` + `idl/` (clip/scroll/effects/text-input protocol)
- `userspace/ui/effects/` (new)
- `source/services/imed/` (new)
- `userspace/ui/renderer/` (caret/selection drawing helpers)
- `tests/ui_v3b_host/` (new)
- `source/apps/selftest-client/` (markers)
- `tools/postflight-ui-v3.sh` (delegates)
- `docs/dev/ui/foundations/layout/scroll.md` + `docs/dev/ui/input/text-input.md` (new)
- `docs/rfcs/RFC-0057-ui-v3a-layout-engine-pretext-contract.md` (layout contract reference)

## Plan (small PRs)

1. **Clipping + scroll**
   - IDL: `SetClip`, `SetScroll`
   - damage math for scroll and clip operating on v3a layout boxes
   - consume the `TASK-0056C` present/input floor instead of introducing a parallel scroll loop
   - verify scroll = place-only: no text reshaping, no layout remeasurement
   - markers: `windowd: clipping on`, `windowd: scroll on`, `windowd: live scroll ok`

2. **Effects**
   - `ui/effects` blur/shadow + cache + budgets
   - markers: `windowd: effects on`, `windowd: effect blur ok`, `windowd: effect budget tripped`

3. **IME/text input**
   - windowd focus → text input subscription
   - caret/selection rendering helpers using v3a line layout metrics
   - if IME v2 Part 1 is present: wire to real `imed` (`TASK-0147`)
   - otherwise: keep explicit stub markers (never claim "full IME")

4. **Proof + docs**
   - host tests + OS selftest + postflight + docs
