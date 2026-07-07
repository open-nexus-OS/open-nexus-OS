<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# SystemUI → DSL migration

Status ledger for moving the system's own chrome onto the Nexus UI DSL.

## Landed

- **TASK-0080B (2026-07-07, host-first)**: bootstrap shell + login greeter as
  real `.nx` project trees:
  - `userspace/systemui/shells/desktop/` — ShellPage (wallpaper/chrome +
    Apps entry) + LauncherPage (registry-driven grid via
    `svc.bundlemgr.enumerate`, service-side search, `svc.ability.launch` on
    tap) + phone override (single-column list).
  - `userspace/systemui/greeter/` — GreeterPage (user list via
    `svc.session.users`, secret field, submit via `svc.session.login`);
    phases idle/authenticating/failure mirror the TASK-0065B contract
    (`docs/dev/ui/shell/session.md`) — **authority stays in sessiond**.
  - Host proofs: `tests/systemui_bootstrap_shell_host/` (profile matrix,
    transcripted launch/login byte-exact, lint/a11y gate).
  - Pattern chapter: `docs/dev/dsl/patterns.md` → "System surfaces".

## Next (TASK-0080C)

- OS wiring: shell mount via the product→profile→shell chain (ADR-0035
  registry `dsl_root` → these trees), greeter→login→shell gate, launcher
  click → RFC-0065 launch → ADR-0042 app surface visible.
- queryd boot wiring (RFC-0069) + postflight script.
- Live input for shell/greeter pages (the in-compositor mount from
  TASK-0076B carries pointer routing already).

## Later

- TASK-0119+: quick settings, notifications, media controls on the
  LauncherPage/ShellPage base.
- Chat/search app migration (own track).
