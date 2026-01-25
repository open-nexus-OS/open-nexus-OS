---
title: TASK-0108 UI v18b: keymintd (software) + keystore encrypted vault + nexus-keychain client (per-user, per-app)
status: Draft
owner: @security
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Identity sessions: tasks/TASK-0107-ui-v18a-identityd-users-sessions.md
  - Device keys baseline: tasks/TASK-0008B-device-identity-keys-v1-virtio-rng-rngd-keystored-keygen.md
  - Persistence (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Policy as Code (namespace guards): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
---

## Context

After we have users and sessions (v18a), we can deliver per-user secrets:

- `keymintd`: software key derivation and key wrapping/signing,
- `keystored`: encrypted per-user vault namespaced per app,
- a small client library (`nexus-keychain`) for apps.

Lock screen and UI flows are separate tasks.

Scope note:

- `TASK-0159`/`TASK-0160` cover **keystored v1.1 hardening** for key lifecycle/rotation/non-exportable ops and attestation/trust store unification.
- This task remains focused on the **per-user encrypted vault** (“keychain”) and session-unseal gating.

## Goal

Deliver:

1. `keymintd` (software):
   - unseal per session (KEK derived from auth token + device salt + user id)
   - Ed25519 signing keys and XChaCha20-Poly1305 wrapping
   - non-exportable keys except wrapped blobs
   - deterministic test mode behind a feature flag
   - marker: `keymintd: ready`
2. `keystored` per-user vault:
   - file `state:/identity/<userId>/vault.kv` (AEAD encrypted)
   - API put/get/del/list scoped to `(sid, appId)` namespace
   - denies access when session is not active/unsealed
   - markers:
     - `keystore: ready`
     - `keystore: put (app=..., key=...)`
3. `userspace/security/nexus-keychain` client:
   - `put/get` helpers that speak to keystore
4. Host tests for vault encryption semantics and session gating deterministically.

## Non-Goals

- Kernel changes.
- Hardware-backed key storage.
- Cross-device sync.

## Constraints / invariants

- Namespace isolation:
  - app cannot read other app’s keys
  - user/session scoping enforced
- Bounded storage per app and per user (caps; documented).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v18b_host/`:

- unseal then put/get/delete secrets works
- access denied when session not active or not unsealed
- cross-app read denied deterministically
- vault persistence survives restart

## Touched paths (allowlist)

- `source/services/keymintd/` (new)
- `source/services/keystored/` (extend or new per-user vault mode)
- `userspace/security/nexus-keychain/` (new)
- `tests/ui_v18b_host/`
- `docs/security/keymint-keychain.md` (new)

## Plan (small PRs)

1. keymintd unseal + wrap/sign + markers + host tests
2. keystored vault + namespace enforcement + markers + host tests
3. nexus-keychain client + docs
