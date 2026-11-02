---
schema: 1
id: TASK-0110
title: "UI v18d: OOBE + Greeter/Login + Accounts app + SystemUI session wiring + OS selftests/postflight"
status: draft
owner: ["@ui"]
created: 2025-12-23
subsystem: sessions
tracks: [TRACK-DSL-V1-DEVX]
links:
  - label: Vision
    target: docs/architecture/vision.md
  - label: Playbook
    target: CLAUDE.md
  - label: "DSL app platform"
    target: tasks/TASK-0122B-dsl-app-platform-v1-shell-routing-launch-contract.md
  - label: "DSL syntax/layout convention"
    target: tasks/TASK-0075-dsl-v0_1a-syntax-ir-cli.md
  - label: Identity
    target: tasks/TASK-0107-ui-v18a-identityd-users-sessions.md
  - label: Keychain
    target: tasks/TASK-0108-ui-v18b-keymintd-keystore-keychain.md
  - label: "Lock screen"
    target: tasks/TASK-0109-ui-v18c-lockd-lockscreen-autolock.md
  - label: "Prefs (idle timeout knob optional)"
    target: tasks/TASK-0072-ui-v9b-prefsd-settings-panels-quick-settings.md
  - label: "Testing contract"
    target: scripts/qemu-test.sh
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
   - visible UI is authored directly in the DSL:
     - `ui/pages/OobePage.nx`
     - `ui/components/**.nx`
     - `ui/composables/**.store.nx`
     - `ui/services/**.service.nx` for identity/session effect adapters
   - minimal wizard to create first user (fast-path for selftests)
   - marker: `oobe: complete (user=uX)`
2. Greeter/Login app (`userspace/apps/login`):
   - visible UI is authored directly in the DSL:
     - `ui/pages/GreeterPage.nx`
     - `ui/components/**.nx`
     - `ui/composables/**.store.nx`
     - `ui/services/**.service.nx` for identity/keymint/session effect adapters
   - Greeter/Login should share shell-state conventions with the lockscreen rather than becoming a second auth shell
   - list users
   - password/PIN login
   - starts session via identityd
   - requests keymint unseal for the session
   - marker: `greeter: open`, `greeter: login ok (user=uX sid=...)`
3. Accounts app (`userspace/apps/accounts`):
   - visible UI is authored directly in the DSL using the shared app platform conventions
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
- `userspace/apps/oobe/ui/` + `userspace/apps/login/ui/` + `userspace/apps/accounts/ui/` (DSL pages/components/composables/services)
- SystemUI bootstrap/wiring for sessions/lock
- `source/apps/selftest-client/`
- `tools/postflight-ui-v18.sh` (delegates)
- `docs/dev/ui/patterns/identity-and-trust/oobe-greeter-lock.md` + `docs/identity/overview.md` + `docs/security/keymint-keychain.md` (extend)

## Plan (small PRs)

1. OOBE fast-path + markers
2. Greeter login flow + session switch wiring + markers
3. Accounts app + keychain UI + markers
4. Selftests + postflight + docs
