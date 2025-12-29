---
title: TASK-0109 UI v18c: lockd auto-lock + lockscreen UI overlay + notifications redaction + media pause hooks
status: Draft
owner: @security
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Identity sessions: tasks/TASK-0107-ui-v18a-identityd-users-sessions.md
  - Keychain session scoping: tasks/TASK-0108-ui-v18b-keymintd-keystore-keychain.md
  - Notifications baseline: tasks/TASK-0069-ui-v8a-notifications-v2-actions-inline-reply.md
  - Media sessions hook (pause on lock): tasks/TASK-0101-ui-v16c-media-sessions-systemui-controls.md
  - Config broker (idle timeout): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
---

## Context

Sessions need a lock state with privacy guarantees:

- auto-lock on input idle,
- explicit lock now,
- unlock via PIN/password using identityd,
- redaction of notification content while locked,
- pause media on lock (best-effort).

OOBE/greeter/account management is separate (v18d).

## Goal

Deliver:

1. `lockd` service:
   - states: Locked/Unlocked(sid)
   - API: `lockNow`, `unlockWith(userId,secret)`, `state`
   - subscribes to input idle ticks (from windowd) to auto-lock
   - markers:
     - `lockd: ready`
     - `lock: now`
     - `lock: unlocked`
2. SystemUI lockscreen overlay:
   - shows time/date, redacted notifications
   - unlock affordances (PIN pad + password field)
   - markers:
     - `lockui: show`
     - `lockui: unlock ok`
3. Integrations:
   - pause media via mediasessd.control("pause") when locking (if present)
   - redaction policy for notifications while locked
4. Host tests:
   - idle timeout triggers lock deterministically (clock injected)
   - wrong PIN backoff (deterministic sequence) if implemented in v18c

## Non-Goals

- Kernel changes.
- Full secure input path (no TEE; userspace only).

## Constraints / invariants

- Deterministic idle timing in tests.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Lock state must gate access to sensitive UI (notifications content redacted).

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v18c_host/`:

- lockNow sets locked state
- idle timeout transitions to locked
- unlockWith succeeds with correct secret and fails deterministically otherwise

### Proof (OS/QEMU) — gated

UART markers:

- `lockd: ready`
- `lock: now`
- `lock: unlocked`
- `lockui: unlock ok`
- `SELFTEST: ui v18 lock ok` (v18d selftest owns full flow)

## Touched paths (allowlist)

- `source/services/lockd/` (new)
- SystemUI lockscreen overlay
- `tests/ui_v18c_host/`
- `docs/ui/oobe-greeter-lock.md` (new/extend)

## Plan (small PRs)

1. lockd core + idle integration + markers + host tests
2. lockscreen overlay + redaction + markers
3. docs
