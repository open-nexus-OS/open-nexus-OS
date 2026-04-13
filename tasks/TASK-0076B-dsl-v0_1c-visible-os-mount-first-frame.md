---
title: TASK-0076B DSL v0.1c: visible OS mount + first DSL frame in windowd/SystemUI
status: Draft
owner: @ui
created: 2026-03-28
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - DSL v0.1b interpreter baseline: tasks/TASK-0076-dsl-v0_1b-interpreter-snapshots-os-demo.md
  - Visible present baseline: tasks/TASK-0055C-ui-v1d-windowd-visible-present-systemui-first-frame.md
  - Visible input baseline: tasks/TASK-0056B-ui-v2a-visible-input-cursor-focus-click.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

`TASK-0076` makes the DSL real through interpretation, snapshots, and an OS demo hook, but its OS proof is still
marker-centric. To turn the DSL into a practical integration tool, we need the first **visible DSL page** mounted in
the live shell and shown in the QEMU graphics window.

This task is the final bridge before Launcher/SystemUI can move to DSL as a real visible shell technology.

## Goal

Deliver:

1. Visible DSL demo mount:
   - mount a DSL page into a managed window or SystemUI surface
   - render its first frame through the real interpreter/runtime path
2. Visible deterministic interaction proof:
   - one bounded interaction (e.g. button tap or state toggle) updates the visible DSL surface
3. Handoff to SystemUI DSL phases:
   - prove that the same mount/runtime path can host Launcher and later Settings/Notifications

## Non-Goals

- Full Launcher migration.
- Quick Settings migration.
- AOT codegen or perf comparisons.
- Large app UI suites.

## Constraints / invariants (hard requirements)

- No separate DSL preview renderer; use the live interpreter/runtime path.
- Follow the canonical `TASK-0075` app layout and `Store`/`Event`/`reduce`/`@effect`/`Page` model.
- Service IO must remain in effects only.
- Visual proof must be deterministic and bounded.

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — required

UART markers:

- `dsl: visible mount on`
- `dsl: first frame visible`
- `dsl: interaction visible ok`
- `SELFTEST: dsl visible mount ok`

Visual proof:

- the QEMU window shows a DSL-rendered page
- a bounded interaction visibly changes the page

## Touched paths (allowlist)

- `userspace/dsl/nx_interp/`
- SystemUI DSL demo mount points
- `userspace/apps/examples/dsl_hello/`
- `source/apps/selftest-client/`
- `docs/dev/dsl/testing.md`
- `docs/systemui/dsl-migration.md`

## Plan (small PRs)

1. visible mount plumbing
2. visible interaction proof page
3. selftests + docs + handoff to `TASK-0080B`/`TASK-0119`
