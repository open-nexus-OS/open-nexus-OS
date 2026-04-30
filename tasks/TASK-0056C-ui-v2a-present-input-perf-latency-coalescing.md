---
title: TASK-0056C UI v2a extension: present/input perf polish (click-to-frame latency + event coalescing + short-circuit compose)
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

## Goal

Deliver a focused present/input perf polish slice:

1. **Click-to-frame latency tightening**:
   - reduce the time from live pointer/click delivery to visible frame update,
   - add stable counters for the common path.
2. **Event coalescing**:
   - coalesce live pointer-motion bursts deterministically,
   - keep click/focus semantics correct while reducing redundant work.
3. **Short-circuit compose/present rules**:
   - no damage and no visible state change → skip compose/present deterministically,
   - unchanged surfaces and idle input should not trigger avoidable work.
4. **Common-case caches**:
   - lightweight hit-test/focus shortcuts where correctness is obvious,
   - fence/wakeup collapse only where semantics stay explicit.

## Non-Goals

- New full input device stacks; consume the live QEMU input path from `TASK-0252`/`TASK-0253`.
- Blur, glass, or backdrop work (handled by `TASK-0059` / `TASK-0060B`).
- Full window manager behavior.
- Kernel redesign; consume the `TASK-0054B/C/D` floor if present.

## Constraints / invariants (hard requirements)

- Preserve `TASK-0056` present and focus semantics.
- Preserve `TASK-0056B` visible affordance semantics and `TASK-0253` live pointer/keyboard semantics.
- Event coalescing must be deterministic and bounded.
- No “fast path” that skips hit-testing correctness for clicks/focus.
- No latency marker can pass on selftest-only input if the live pointer path regresses.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v2c_host/` or equivalent:

- pointer burst is coalesced deterministically,
- live QEMU pointer burst is coalesced without losing the latest visible cursor position,
- click causes one visible frame update without redundant extra presents,
- no-damage / unchanged-state path skips compose/present,
- focus correctness is unchanged.

### Proof (OS/QEMU) — gated

UART markers (order tolerant):

- `windowd: present fastpath on`
- `windowd: pointer coalesce ok`
- `windowd: no-damage skip ok`
- `windowd: click latency ok`
- `SELFTEST: live pointer latency ok`
- `SELFTEST: ui v2 perf ok`

## Touched paths (allowlist)

- `source/services/windowd/`
- `userspace/apps/launcher/` (or other small proof surface)
- `tests/ui_v2c_host/` (new)
- `source/apps/selftest-client/`
- `docs/dev/ui/input/input.md`
- `docs/dev/ui/foundations/rendering/renderer.md`
- `docs/dev/ui/foundations/quality/testing.md`

## Plan (small PRs)

1. add common-case present/input counters and short-circuit rules
2. add deterministic pointer burst coalescing
3. tighten click-to-frame visible update path
4. add host/QEMU proof scenes and docs
