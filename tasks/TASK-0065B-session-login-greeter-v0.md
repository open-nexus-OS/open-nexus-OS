---
title: TASK-0065B Session/Login v0: greeter/dev-session + SystemUI shell handoff
status: Draft
owner: @ui @platform
created: 2026-04-30
depends-on: []
follow-up-tasks: []
links:
  - UI v6b app lifecycle: tasks/TASK-0065-ui-v6b-app-lifecycle-notifications-navigation.md
  - UI v6a window management: tasks/TASK-0064-ui-v6a-window-management-scene-transitions.md
  - Visible input baseline: tasks/TASK-0056B-ui-v2a-visible-input-cursor-focus-click.md
  - Live input pipeline: tasks/TASK-0253-input-v1_0b-os-hidrawd-touchd-inputd-ime-hooks-selftests.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

The UI fast lane needs a small session/login floor before SystemUI can claim a desktop
experience. This task provides a greeter/dev-session handoff without turning login into
a kernel claim or a full account system.

The target is Orbital-Level UX, not Orbital architecture: session state is brokered by a
small userspace service, SystemUI owns the visible shell surfaces, and `windowd` remains
the input/focus/window authority.

## Goal

Deliver:

1. Session broker v0:
   - start a dev-session or minimal local login flow,
   - expose session ready/locked/unlocked state to SystemUI,
   - reject unauthorized app launch before session ready.
2. Greeter/SystemUI surface:
   - visible greeter or dev-session splash,
   - live pointer focus/click handoff into the shell via `TASK-0253` + `windowd`,
   - minimal keyboard path only as needed for the greeter.
3. Shell handoff:
   - after session ready, launcher/dock/taskbar surface is visible,
   - app launch requests are delegated to `appmgrd`.
4. Host tests and OS/QEMU markers.

## Non-Goals

- Full multi-user accounts, password storage, PAM, biometric auth, or remote login.
- Kernel-enforced sessions.
- Full keyboard/IME stack beyond the minimal greeter path.

## Constraints / invariants (hard requirements)

- Session state is a userspace authority; SystemUI may render it but must not forge it.
- `windowd` remains the input/hit-test/focus authority.
- Apps cannot receive global pointer/keyboard events from the greeter.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_session_host/`:

- pre-session app launch is rejected,
- session ready enables launcher/app launch requests,
- lock/unlock state changes are deterministic,
- greeter handoff preserves focus ownership.

### Proof (OS/QEMU) — gated

UART markers (order tolerant):

- `sessiond: ready`
- `systemui: greeter visible`
- `sessiond: session ready`
- `systemui: shell visible`
- `SELFTEST: ui session ok`

### Visual proof — required

- QEMU shows the greeter/dev-session surface first.
- Live pointer click advances to the shell/launcher surface.
- App launch after session ready uses the `TASK-0065` app lifecycle path.

## Touched paths (allowlist)

- `source/services/sessiond/` (new)
- `source/services/systemui/`
- `source/services/windowd/` (integration only)
- `source/services/appmgrd/` (policy integration only)
- `tests/ui_session_host/` (new)
- `source/apps/selftest-client/`
- `docs/dev/ui/shell/session.md` (new)

## Plan (small PRs)

1. session model + host reject/allow tests
2. greeter/dev-session surface + markers
3. SystemUI shell handoff + appmgrd integration
4. QEMU visual proof + docs
