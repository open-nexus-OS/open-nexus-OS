---
title: TASK-0080C SystemUI DSL bootstrap shell (OS/QEMU): visible launcher mount + app launch selftests
status: Draft
owner: @ui
created: 2026-03-28
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Bootstrap shell host phase: tasks/TASK-0080B-systemui-dsl-bootstrap-shell-launcher-host.md
  - Dev display/profile presets follow-up: tasks/TASK-0055D-ui-v1e-dev-display-profile-presets-qemu-hz.md
  - Visible input baseline: tasks/TASK-0056B-ui-v2a-visible-input-cursor-focus-click.md
  - SystemUI DSL migration phase 1b follow-up: tasks/TASK-0120-systemui-dsl-migration-phase1b-os-wiring-postflight.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Host launcher proofs are not enough for app testing. We need the bootstrap shell from `TASK-0080B` mounted into the
live OS so that Files/Text/Images and other app tasks can launch from a visible desktop before the broader SystemUI DSL
phases land.

This task is the first OS-mounted Orbital-Level shell proof: live QEMU pointer input must
hover and click launcher entries, launch a real app window, and preserve the single
SystemUI shell path.

## Goal

Deliver:

1. Visible bootstrap shell mount in OS:
   - SystemUI boots into the DSL bootstrap shell by default in the early UI profile
   - launcher is visible in the QEMU window
   - the mounted shell consumes the same profile/orientation device environment that later canonical SystemUI DSL phases use
   - shell appears after the `TASK-0065B` greeter/dev-session handoff when that gate is active
2. Real launch integration:
   - live QEMU pointer hover/click selects a launcher entry and launches a real app window
   - return/focus behavior is deterministic enough for selftests
3. Marker-driven proof for app bring-up:
   - launcher visible
   - app launch initiated
   - launched app frame appears

## Non-Goals

- Full Quick Settings mount.
- Full SystemUI migration.
- Full session-aware launcher variants; only the greeter/dev-session handoff gate is required if `TASK-0065B` has landed.

## Constraints / invariants (hard requirements)

- Keep one SystemUI shell path; no temporary alternate desktop app.
- Use feature flags only as bounded migration aids.
- Markers must reflect real visible launcher mount and real app launch.
- Launch success markers must require live routed pointer input, not selftest-only launch mutation.

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — required

UART markers:

- `systemui:dsl bootstrap shell on`
- `systemui:dsl launcher visible`
- `systemui:dsl launcher hover visible`
- `launcher: app launch request ok`
- `launcher: app frame visible`
- `SELFTEST: systemui live launcher click ok`
- `SELFTEST: systemui bootstrap launcher ok`

Visual proof:

- launcher is visible in the QEMU window
- moving the host pointer over a launcher entry shows hover/focus state
- clicking the launcher entry opens a visible app frame

## Touched paths (allowlist)

- SystemUI mount points / bootstrap selection
- launcher app integration
- `source/apps/selftest-client/`
- `tools/postflight-systemui-bootstrap-shell.sh` (new)
- `docs/systemui/dsl-migration.md`

## Plan (small PRs)

1. OS mount + feature flag
2. launch/focus selftests
3. postflight + docs
