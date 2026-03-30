---
title: TASK-0062B UI v5a extension: animation frame-budget discipline + perf scenes + QEMU fluidity gates
status: Draft
owner: @ui @runtime
created: 2026-03-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - UI v5a runtime/animation baseline: tasks/TASK-0062-ui-v5a-reactive-runtime-animation-transitions.md
  - Glass compositor follow-up: tasks/TASK-0060B-ui-v4b-glass-materials-backdrop-cache-degrade.md
  - Compositor perf baseline: tasks/TASK-0060-ui-v4a-tiled-compositor-clipstack-atlases-perf.md
  - Perf tracing follow-ups: tasks/TASK-0143-perf-v1a-perfd-frame-trace-metrics.md
  - Deterministic perf pipeline: tasks/TASK-0144-perf-v1b-instrumentation-hooks-hud-nx-perf.md
  - Perf gates: tasks/TASK-0145-perf-v1c-deterministic-gates-key-scenes.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

`TASK-0062` establishes the functional animation/runtime substrate:

- retained reactive runtime,
- vsync timeline,
- keyframes and springs,
- implicit transitions.

This follow-up turns animation into an explicit **performance gate**. The goal is not just “transitions exist”, but
that layered motion remains fluid in QEMU so it becomes an honest system-wide test for:

- scheduling and wakeups,
- present pacing,
- damage/caching discipline,
- and glass/effect degradation under load.

## Goal

Deliver animation performance discipline and canonical scenes:

1. **Frame-budget discipline**:
   - define a bounded per-frame animation work budget,
   - coalesce redundant animation/property updates within a frame.
2. **Overload behavior**:
   - deterministic degrade rules when animation work exceeds budget,
   - reduced-motion and low-power remain first-class policy inputs.
3. **Canonical perf scenes**:
   - glass sidebar open/close,
   - translucent control-center or sheet over active background,
   - launcher/hover/focus transitions,
   - modal / notification motion scenes.
4. **QEMU fluidity gates**:
   - treat these scenes as repeatable, bounded performance checks rather than subjective feel only.

## Non-Goals

- A cinematic animation framework with arbitrary effects.
- GPU-specific animation paths.
- Replacing the runtime semantics established by `TASK-0062`.
- Whole-system perf tooling replacement (follow-ons `TASK-0143/0144/0145` stay authoritative for deeper tracing).

## Constraints / invariants (hard requirements)

- Animation ordering and sampling remain deterministic.
- Budget/degrade logic must be explicit and testable.
- No unbounded spring/keyframe work accumulation.
- Reduced motion must continue to short-circuit or shorten transitions deterministically.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v5b_perf_host/`:

- redundant animation property changes coalesce into bounded frame work,
- frame-budget exceed path degrades deterministically,
- canonical scenes produce stable counters / pass-fail outcomes,
- reduced motion scenes remain functionally correct with smaller work.

### Proof (OS/QEMU) — gated

UART markers (order tolerant):

- `uianim: budget on`
- `uianim: coalesce ok`
- `uianim: scene sidebar ok`
- `uianim: scene glass sheet ok`
- `SELFTEST: ui v5 perf ok`

## Touched paths (allowlist)

- `userspace/ui/runtime/`
- `userspace/ui/animation/`
- `source/services/windowd/`
- `tests/ui_v5b_perf_host/` (new)
- `source/apps/selftest-client/`
- `docs/dev/ui/animation.md`
- `docs/dev/ui/testing.md`

## Plan (small PRs)

1. define animation frame-budget and coalescing rules
2. wire deterministic overload/degrade behavior
3. add canonical glass-and-motion perf scenes
4. add host/QEMU gates that make fluidity a tracked contract
