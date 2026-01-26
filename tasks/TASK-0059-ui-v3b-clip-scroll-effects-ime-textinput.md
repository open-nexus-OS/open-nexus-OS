---
title: TASK-0059 UI v3b: clipping/scroll layers + precise damage + CPU effects (blur/shadow) + IME/text-input stub
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - UI v3a layout/wrap: tasks/TASK-0058-ui-v3a-layout-wrapping-deterministic.md
  - UI v2a present/input baseline: tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md
  - UI v2b shaping/svg baseline: tasks/TASK-0057-ui-v2b-text-shaping-svg-pipeline.md
  - UI v1b windowd baseline: tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md
  - Drivers/Accelerators contracts: tasks/TRACK-DRIVERS-ACCELERATORS.md
  - Config broker (budgets): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Policy as Code (IME focus guards): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

With layout/wrapping available (v3a), v3b adds:

- correct clipping + scroll layers with precise damage math,
- CPU composition effects with budgets (blur/shadow),
- a minimal IME/text-input stub path (composition/commit, caret/selection).

This task is QEMU-tolerant but has more moving parts, so it is gated on prior UI v1/v2 tasks.

## Goal

Deliver:

1. `windowd` clipping + scroll layers:
   - scissor clipping
   - scroll offsets and scroll damage rules
2. CPU effects module:
   - separable blur and drop shadow
   - caching and per-frame budgets with deterministic degrade behavior
3. Minimal IME/text-input:
   - IME/text-input protocol plumbing (focus routing + caret/selection integration)
   - `imed` is introduced as a stub only if IME v2 tasks are not yet landed
   - text-input protocol for focused surface
   - caret/selection rendering helpers
4. Host tests (damage/effects/IME flow) and OS markers + postflight.

## Non-Goals

- Kernel changes.
- Full IME engine (language models, dictionaries). v3b is protocol plumbing only.
- US/DE keymaps, dead keys/compose tables, OSK overlay, and IME host behavior (tracked as IME/Text v2 Part 1: `TASK-0146`/`TASK-0147`).
- Clipboard daemon creation if it doesn’t exist yet (integration is optional and gated).

## Constraints / invariants (hard requirements)

- Deterministic damage math and deterministic effect outputs.
- Strict budgets:
  - cap blur radius and area per frame,
  - cap cached effect entries and total bytes.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Clear “what is stubbed” for IME and clipboard integration.

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
- clipping:
  - clipped layers do not paint outside clip; damage respects clip
- effects:
  - blur/shadow output matches goldens (pixel-exact or SSIM threshold)
  - budget trip path degrades deterministically and records a marker/counter
- IME:
  - mocked IME compose/commit updates a model and renders caret/selection correctly

### Proof (OS/QEMU) — gated

UART markers (order tolerant):

- `windowd: clipping on`
- `windowd: scroll on`
- `windowd: effects on`
- `windowd: effect blur ok`
- `imed: ready`
- `SELFTEST: ui v3 scroll ok`
- `SELFTEST: ui v3 ime ok`
- `SELFTEST: ui v3 effect ok`
- `SELFTEST: ui v3 wrap ok`

## Touched paths (allowlist)

- `source/services/windowd/` + `idl/` (clip/scroll/effects/text-input protocol)
- `userspace/ui/effects/` (new)
- `source/services/imed/` (new)
- `userspace/ui/renderer/` (caret/selection drawing helpers)
- `tests/ui_v3b_host/` (new)
- `source/apps/selftest-client/` (markers)
- `tools/postflight-ui-v3.sh` (delegates)
- `docs/dev/ui/scroll.md` + `docs/dev/ui/effects.md` + `docs/dev/ui/input-text.md` (new)

## Plan (small PRs)

1. **Clipping + scroll**
   - IDL: `SetClip`, `SetScroll`
   - damage math for scroll and clip
   - markers: `windowd: clipping on`, `windowd: scroll on`, `windowd: scroll present ok`

2. **Effects**
   - `ui/effects` blur/shadow + cache + budgets
   - markers: `windowd: effects on`, `windowd: effect blur ok`, `windowd: effect budget tripped`

3. **IME/text input**
   - windowd focus → text input subscription
   - caret/selection rendering helpers
   - if IME v2 Part 1 is present: wire to real `imed` (`TASK-0147`)
   - otherwise: keep explicit stub markers (never claim “full IME”)

4. **Proof + docs**
   - host tests + OS selftest + postflight + docs
