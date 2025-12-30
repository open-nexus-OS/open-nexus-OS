---
title: TASK-0110 UI v18d: OOBE + Greeter/Login + Accounts app + SystemUI session wiring + OS selftests/postflight
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Identity: tasks/TASK-0107-ui-v18a-identityd-users-sessions.md
  - Keychain: tasks/TASK-0108-ui-v18b-keymintd-keystore-keychain.md
  - Lock screen: tasks/TASK-0109-ui-v18c-lockd-lockscreen-autolock.md
  - Prefs (idle timeout knob optional): tasks/TASK-0072-ui-v9b-prefsd-settings-panels-quick-settings.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Identity features only matter if the system can:

- bootstrap first boot (OOBE) when no users exist,
- present a greeter/login flow,
- wire SystemUI to session start/end and lock/unlock,
- provide an accounts management UI,
- and prove all of it in QEMU markers.

This task owns the end-to-end OS selftests and postflight for UI v18.

## Goal

Deliver:

1. OOBE app (`userspace/apps/oobe`):
   - minimal wizard to create first user (fast-path for selftests)
   - marker: `oobe: complete (user=uX)`
2. Greeter/Login app (`userspace/apps/login`):
   - list users
   - password/PIN login
   - starts session via identityd
   - requests keymint unseal for the session
   - marker: `greeter: open`, `greeter: login ok (user=uX sid=...)`
3. Accounts app (`userspace/apps/accounts`):
   - change pass/pin
   - keychain list/delete per app namespace (minimal)
   - markers:
     - `accounts: change pass ok`
     - `accounts: pin set`
     - `accounts: keychain del (app=... key=...)`
4. SystemUI wiring:
   - first boot auto-starts OOBE if no users
   - after login, SystemUI switches into the session desktop
   - lock state synced with lockd
   - quick settings tile “Lock” (stub)
5. OS selftests:
   - create user (if needed), login, put/get keychain, lock/unlock
6. Postflight `postflight-ui-v18.sh` (delegating) and docs.

## Non-Goals

- Kernel changes.
- Remote account auth.

## Constraints / invariants

- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Deterministic selftest fast-paths (bounded timeouts; no busy-wait).

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v18_host/` can remain in v18a/b/c tasks, but v18d must ensure end-to-end pieces are wired.

### Proof (OS/QEMU) — required once gated deps exist

UART markers:

- `identityd: ready`
- `keymintd: ready`
- `keystore: ready`
- `lockd: ready`
- `oobe: complete (user=...)`
- `greeter: login ok`
- `SELFTEST: ui v18 login ok`
- `SELFTEST: ui v18 keychain ok`
- `SELFTEST: ui v18 lock ok`

## Touched paths (allowlist)

- `userspace/apps/oobe/` (new)
- `userspace/apps/login/` (new)
- `userspace/apps/accounts/` (new)
- SystemUI bootstrap/wiring for sessions/lock
- `source/apps/selftest-client/`
- `tools/postflight-ui-v18.sh` (delegates)
- `docs/ui/oobe-greeter-lock.md` + `docs/identity/overview.md` + `docs/security/keymint-keychain.md` (extend)

## Plan (small PRs)

1. OOBE fast-path + markers
2. Greeter login flow + session switch wiring + markers
3. Accounts app + keychain UI + markers
4. Selftests + postflight + docs

