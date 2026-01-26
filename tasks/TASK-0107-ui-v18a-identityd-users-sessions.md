---
title: TASK-0107 UI v18a: identityd (users + password/PIN auth + sessions) with Argon2id + persistence + tests
status: Draft
owner: @security
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Persistence (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Policy as Code (identity guards): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Config broker (argon2 params): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Testing contract: scripts/qemu-test.sh
  - Data formats rubric (JSON vs Cap'n Proto): docs/adr/0021-structured-data-formats-json-vs-capnp.md
---

## Context

Identity & Sessions is a user-visible feature, but it’s also a security boundary. We start with a minimal,
deterministic identity service:

- users stored under `/state`,
- password/PIN auth with Argon2id,
- session start/end and “active session” query.

Keychain, lockscreen, and UI flows are separate tasks.

## Goal

Deliver:

1. `identityd` service:
   - user DB persisted at `state:/identity/users.nxs` (Cap'n Proto snapshot; canonical)
     - optional derived/debug view: `nx identity export --json` emits deterministic JSON
   - list/create/remove users
   - set password/PIN
   - auth (password/pin) returning a session token
   - start/end session and `active()`
   - markers:
     - `identityd: ready`
     - `identity: user created`
     - `identity: session start (sid=...)`
     - `identity: session end (sid=...)`
2. Argon2id hashing (pure Rust):
   - parameters persisted with the user record
   - configurable via config (host tests use fixed parameters)
3. Host tests for auth/session determinism and persistence.

## Non-Goals

- Kernel changes.
- Full multi-factor auth.
- Network accounts.

## Constraints / invariants

- Deterministic tests:
  - inject clock and RNG seeds for tests
  - stable DB serialization ordering.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Bounded resources:
  - cap user count and name lengths
  - cap auth token length.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v18a_host/`:

- create user, auth with password and pin succeeds
- wrong secret fails deterministically
- start/end session transitions stable
- persistence: restart identityd and users remain

### Proof (OS/QEMU) — gated

UART markers:

- `identityd: ready`

## Touched paths (allowlist)

- `source/services/identityd/` (new)
- `tests/ui_v18a_host/`
- `docs/identity/overview.md` (new)

## Plan (small PRs)

1. identityd core + IDL + markers
2. Argon2id integration + config knobs + host tests
3. docs

Follow-up:

- Accounts/Identity v1.2 (multi-user + lockout policy + session manager fast-switch + SecureFS home + per-user keystore binding + Greeter/OOBE wiring) is tracked as `TASK-0223`/`TASK-0224`.
