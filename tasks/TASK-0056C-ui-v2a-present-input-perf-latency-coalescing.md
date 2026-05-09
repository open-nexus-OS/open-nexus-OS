---
title: TASK-0056C UI v2a extension: embedded reactor/runtime floor + present/input perf polish (input-to-frame latency + event coalescing + short-circuit compose)
status: Draft
owner: @ui @runtime
created: 2026-03-29
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - UI v2a baseline: tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md
  - Visible input bridge: tasks/TASK-0056B-ui-v2a-visible-input-cursor-focus-click.md
  - UI perf floor baseline: tasks/TASK-0054B-ui-v1a-kernel-ui-perf-floor-zero-copy-qos-hardening.md
  - Kernel IPC fastpath: tasks/TASK-0054C-ui-v1a-kernel-ipc-fastpath-control-plane-vmo-bulk.md
  - Kernel MM perf floor: tasks/TASK-0054D-ui-v1a-kernel-mm-perf-floor-vmo-surface-reuse.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

`TASK-0056` proves the first real-time UX semantics:

- double buffering,
- vsync-aligned present,
- input hit-testing and focus.

`TASK-0056B` owns the deterministic visible-input proof. `TASK-0252`/`TASK-0253`
then provide the live QEMU pointer/keyboard path. This follow-up exists after
that input pipeline to make the path feel responsive enough for the
Orbital-Level UX gate before scrolling, animation, window management, and
launcher work build on it.

This task is the embedded **reactor/runtime minimum floor** for the UI fast lane.
It should tighten the common path across `inputd` -> `fbdevd` -> `windowd`
without creating a detached parallel subsystem. `TASK-0059`, `TASK-0062`,
`TASK-0063`, and `TASK-0064` must extend this floor rather than re-invent it.

## Goal

Deliver the minimum runtime/reactor floor and a focused present/input perf polish slice:

1. **Common-case input-to-frame latency tightening**:
   - reduce the time from live pointer/click/wheel/key delivery to visible frame update,
   - add stable counters for the common path.
2. **Event coalescing**:
   - coalesce live pointer-motion bursts deterministically within and across present cadence,
   - keep click/focus/wheel/key semantics correct while reducing redundant work.
3. **Short-circuit compose/present rules**:
   - no damage and no visible state change → skip compose/present deterministically,
   - unchanged surfaces and idle input should not trigger avoidable work,
   - idle desktop path should settle into a low-work state.
4. **Common-case caches / wakeup collapse**:
   - lightweight hit-test/focus shortcuts where correctness is obvious,
   - fence/wakeup collapse only where semantics stay explicit,
   - avoid unnecessary visible-state fetch / compose / present work in the common case.
5. **Embedded handoff to later fast-lane tasks**:
   - establish explicit counters, damage rules, and present reasons that `TASK-0059`,
     `TASK-0062`, `TASK-0063`, and `TASK-0064` can extend.

## Non-Goals

- New full input device stacks; consume the live QEMU input path from `TASK-0252`/`TASK-0253`.
- A separate standalone runtime/platform track outside the UI fast lane.
- Blur, glass, or backdrop work (handled by `TASK-0059` / `TASK-0060B`).
- Full window manager behavior.
- Kernel redesign; consume the `TASK-0054B/C/D` floor if present.

## Constraints / invariants (hard requirements)

- Preserve `TASK-0056` present and focus semantics.
- Preserve `TASK-0056B` visible affordance semantics and `TASK-0253` live pointer/keyboard semantics.
- Event coalescing must be deterministic and bounded.
- No “fast path” that skips hit-testing correctness for clicks/focus.
- Preserve service ownership boundaries: `inputd` owns normalized input state, `fbdevd` owns display polling/present loop, `windowd` owns hit-test/focus/present semantics.
- Observer/proof latching must not become sticky render-state behavior.
- No latency marker can pass on selftest-only input if the live pointer path regresses.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v2c_host/` or equivalent:

- pointer burst is coalesced deterministically,
- live QEMU pointer burst is coalesced without losing the latest visible cursor position,
- click/wheel/key state changes each cause at most one visible frame update per cadence in the common case,
- no-damage / unchanged-state path skips avoidable fetch/compose/present work,
- idle path stays quiet and exposes stable low-work counters,
- focus correctness is unchanged.

### Proof (OS/QEMU) — gated

UART markers (order tolerant):

- `windowd: present fastpath on`
- `windowd: pointer coalesce ok`
- `windowd: no-damage skip ok`
- `windowd: idle fastpath ok`
- `windowd: click latency ok`
- `SELFTEST: live pointer latency ok`
- `SELFTEST: ui v2 perf ok`

## Touched paths (allowlist)

- `source/services/windowd/`
- `source/services/fbdevd/`
- `source/services/inputd/`
- `userspace/input-live-protocol/`
- `userspace/apps/launcher/` (or other small proof surface)
- `tests/ui_v2c_host/` (new)
- `source/apps/selftest-client/`
- `docs/dev/ui/input/input.md`
- `docs/dev/ui/foundations/rendering/renderer.md`
- `docs/dev/ui/foundations/quality/testing.md`

## Plan (small PRs)

1. add common-case chain counters and short-circuit rules across `inputd` / `fbdevd` / `windowd`
2. add deterministic pointer-motion burst coalescing without losing latest visible state
3. tighten input-to-frame visible update path and idle cheap behavior
4. add host/QEMU proof scenes and docs
