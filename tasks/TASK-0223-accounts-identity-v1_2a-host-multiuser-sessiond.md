---
title: TASK-0223 Accounts/Identity v1.2a (host-first): multi-user identity + lockout policy + session manager (login/lock/switch) semantics + deterministic tests
status: Draft
owner: @security
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Identity baseline (users + auth + sessions): tasks/TASK-0107-ui-v18a-identityd-users-sessions.md
  - Lock state baseline: tasks/TASK-0109-ui-v18c-lockd-lockscreen-autolock.md
  - OOBE/Greeter baseline: tasks/TASK-0110-ui-v18d-oobe-greeter-accounts-systemui-os-proofs.md
  - SecureFS overlay baseline (OS-gated): tasks/TASK-0183-encryption-at-rest-v1b-os-securefsd-unlock-ui-migration-cli-selftests.md
  - Keystored v1.1 lifecycle/non-exportable ops: tasks/TASK-0159-identity-keystore-v1_1-host-keystored-lifecycle-nonexportable.md
  - Persistence substrate (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Data formats rubric (JSON vs Cap'n Proto): docs/adr/0021-structured-data-formats-json-vs-capnp.md
---

## Context

The repo already tracks “Identity v18” (identityd), lock state (lockd), and OOBE/greeter wiring as separate tasks.
Accounts/Identity v1.2 is a consolidation and hardening step:

- multi-user model (users + groups) and deterministic auth/lockout,
- a first-class session manager that supports login/lock/unlock/fast user switch,
- and well-defined gates for encrypted homes and keystore binding (OS work in v1.2b).

This task is host-first: we define semantics and prove them deterministically without claiming SecureFS security.

## Goal

Deliver:

1. Identity service extensions (prefer extending existing `identityd` rather than introducing a parallel `identityd`):
   - multi-user:
     - stable `uid` allocation and stable list ordering
     - groups list per user (bounded)
   - auth kinds: password and PIN (biometric remains explicit stub)
   - lockout policy:
     - threshold/window/cooldown, injected clock for tests
2. Session manager semantics (new service `sessiond` or integrated broker module):
   - `login(name, kind, secret)` returns a `Session { sid, uid, locked }`
   - `lock(sid)` and `unlock(sid, kind, secret)` deterministic behavior
   - `switch(sid, targetUid)`:
     - creates a new session, locks the prior one, stable ordering rules
   - `logout(sid)` ends and clears state deterministically
3. Deterministic storage strategy (host-first):
   - identity DB can remain JSONL/JSON (existing baseline) or migrate later; do not force libSQL unless explicitly decided
   - host tests use a tempdir storage adapter
4. Deterministic host tests `tests/accounts_identity_v1_2_host/`:
   - create users; login succeeds; wrong secret fails
   - lockout triggers and cooldown restores access deterministically
   - fast switch between two users yields deterministic session states
   - OOBE required when no users exist (policy-only flag)

## Non-Goals

- Kernel changes.
- Encrypted homes implementation details (OS-gated; see v1.2b).
- Claiming security properties that depend on entropy/keystore readiness.

## Constraints / invariants (hard requirements)

- Determinism: injected clock; stable ordering; stable error reasons.
- Bounded state: caps on users, groups, attempts, and session count.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **RED (avoid crypto drift)**:
  - v1.2 must not introduce new crypto.
  - Any “deterministic SIV-style nonces for ChaCha20-Poly1305” idea is out-of-scope and unsafe unless using a misuse-resistant construction.
  - Canonical direction is SecureFS (`TASK-0182/0183`), which explicitly defines:
    - per-file `file_id` + per-file subkey (HKDF(MEK, file_id)),
    - per-file random `nonce_prefix` + chunk counter,
    - AAD bound to `file_id`/metadata (not path),
    - and deterministic/seeded RNG only in tests (labeled insecure).

- **YELLOW (identity storage backend drift)**:
  - Existing identity task uses `state:/identity/users.nxs` + Argon2id.
  - Do not silently switch to libSQL + scrypt without a decision; keep one canonical contract and document migrations explicitly.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p accounts_identity_v1_2_host -- --nocapture`

## Touched paths (allowlist)

- `source/services/identityd/` (extend, or define clear replacement plan)
- `source/services/sessiond/` (new; host-first core acceptable)
- `tests/accounts_identity_v1_2_host/`
- docs may land in v1.2b

## Plan (small PRs)

1. identity lockout semantics + injected clock + tests
2. sessiond model (login/lock/unlock/switch) + tests
3. docs stub notes + follow-up links

## Acceptance criteria (behavioral)

- Host tests deterministically prove multi-user auth, lockout, and fast-switch semantics.
