---
title: TASK-0062 UI v5a: reactive retained runtime + vsync animation timeline + implicit transitions (reduced motion)
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - UI v2a present scheduler baseline: tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md
  - UI v3a layout baseline: tasks/TASK-0058-ui-v3a-layout-wrapping-deterministic.md
  - UI v4a pacing/metrics baseline: tasks/TASK-0060-ui-v4a-tiled-compositor-clipstack-atlases-perf.md
  - Config broker (motion knobs): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Up to UI v4 we mostly operate in an “immediate-ish” style: clients submit buffers and windowd composes.
UI v5 introduces a retained, reactive runtime on top:

- reactive signals/derived/effects,
- deterministic frame-batching,
- an animation timeline driven by vsync,
- implicit transitions on common layer property changes.

This is v5a (runtime + animation + transitions). Virtualized list and theme tokens are v5b (`TASK-0063`).

## Goal

Deliver:

1. `userspace/ui/runtime`:
   - signals/derived/effects
   - deterministic ordering
   - frame-batched commits (coalesce updates per vsync tick)
   - metrics and markers
2. `userspace/ui/animation`:
   - keyframes and springs
   - vsync-driven sampling
   - reduced motion flag support
3. `windowd` implicit transitions:
   - watch layer props (opacity/transform/shadow radius)
   - install default transitions if no explicit animator is active
   - respect reduced motion config

## Non-Goals

- Kernel changes.
- A full widget toolkit (v5a is the runtime foundation).
- Theme tokens and virtualization (v5b).

## Constraints / invariants (hard requirements)

- Deterministic batching and effect ordering (no re-entrancy surprises).
- Bounded work per frame (caps on queued work items).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **YELLOW (runtime complexity)**:
  - Keep runtime minimal and auditable. Avoid a full “react clone”; focus on stable primitives.
- **YELLOW (animation determinism)**:
  - Spring integration must be deterministic across platforms (explicit dt, explicit rounding).

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v5a_host/`:

- reactive runtime:
  - update chain coalesces into one frame commit
  - ordering is stable
- timeline sampling:
  - keyframe reaches target within tolerance at expected time
  - spring converges within tolerance; dropped frames counter remains low
- implicit transitions:
  - property change triggers transition
  - reduced motion disables/shortens transitions deterministically

### Proof (OS/QEMU) — gated

UART markers (order tolerant):

- `uiruntime: on`
- `uiruntime: batch commit ok (nodes=<n>)`
- `uianim: timeline on`
- `windowd: implicit transitions on`
- `SELFTEST: ui v5 transition ok`
- `SELFTEST: ui v5 spring ok`

## Touched paths (allowlist)

- `userspace/ui/runtime/` (new)
- `userspace/ui/animation/` (new)
- `source/services/windowd/` (implicit transitions)
- `tests/ui_v5a_host/` (new)
- `source/apps/selftest-client/` (markers)
- `tools/postflight-ui-v5a.sh` (delegates)
- `docs/dev/ui/runtime.md` + `docs/dev/ui/animation.md` (new)

## Plan (small PRs)

1. runtime primitives + batching + markers
2. timeline (keyframes/springs) + markers
3. windowd implicit transitions + reduced motion
4. host tests + OS markers + docs
