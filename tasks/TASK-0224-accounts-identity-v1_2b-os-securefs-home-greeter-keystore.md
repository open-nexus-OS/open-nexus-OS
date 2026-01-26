---
title: TASK-0224 Accounts/Identity v1.2b (OS/QEMU): sessiond orchestration + SecureFS-backed per-user home mount + per-user keystore unlock + Greeter/OOBE/Lock wiring + selftests/docs
status: Draft
owner: @security
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Accounts/Identity v1.2 host semantics: tasks/TASK-0223-accounts-identity-v1_2a-host-multiuser-sessiond.md
  - SecureFS overlay baseline (encryption-at-rest): tasks/TASK-0183-encryption-at-rest-v1b-os-securefsd-unlock-ui-migration-cli-selftests.md
  - Keystore v1.1 OS wiring (seal/unseal + trust unification): tasks/TASK-0160-identity-keystore-v1_1-os-attestd-trust-unification-selftests.md
  - Identity baseline: tasks/TASK-0107-ui-v18a-identityd-users-sessions.md
  - Lock baseline: tasks/TASK-0109-ui-v18c-lockd-lockscreen-autolock.md
  - OOBE/Greeter baseline: tasks/TASK-0110-ui-v18d-oobe-greeter-accounts-systemui-os-proofs.md
  - Persistence substrate (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

This task wires multi-user identity + sessions into OS/QEMU with encrypted homes and keystore binding.
We must stay honest about security: SecureFS encryption and keystore secrets depend on prerequisites.

## Goal

Deliver:

1. `sessiond` OS orchestration:
   - on successful login:
     - create/activate a session (sid, uid)
     - mount the user home (see below)
     - unlock per-user keystore domain (see below)
     - switch SystemUI into the user session desktop deterministically
   - lock/unlock:
     - integrate with `lockd` (lock screen overlay)
     - unlocking re-checks identity secret deterministically
   - fast switch:
     - create second session; prior remains locked
2. SecureFS-backed per-user home:
   - avoid introducing new crypto:
     - reuse `securefsd` overlay (`state:/secure/**`) from `TASK-0183`
   - SecureFS crypto contract (explicit, to avoid later RED):
     - MUST follow `TASK-0182/0183` (XChaCha20-Poly1305, per-file `file_id`, HKDF-derived per-file subkeys, per-file random nonce prefix + chunk counter; AAD bound to `file_id`/metadata, not path)
     - deterministic AEAD nonce modes are forbidden in production; seeded/test RNG is test-only and must be labeled insecure
   - mount model:
     - per-user home root: `state:/secure/home/<uid>/` (inside SecureFS namespace)
     - sessiond publishes “current home URI” for UI/services (exact mechanism documented)
   - `/state` gating:
     - without `/state` (`TASK-0009`), no persistence claims; selftests must not claim “home persisted”
3. Per-user keystore unlock binding:
   - reuse keystored v1.1 non-exportable operations where possible
   - unlock per-user domain only after a successful login and only for the active session
   - avoid passing raw passwords across services:
     - use a session-scoped derived unlock token (“SUT”) only if it is cryptographically sound and documented
     - otherwise, keystored can derive keys from a sealed secret it stores (preferred direction)
4. Greeter/OOBE and lock screen wiring:
   - OOBE creates the first user if none exist
   - Greeter lists users, login via password/PIN, starts session
   - Lock screen engages on lockd state
   - markers:
     - `ui: oobe start`
     - `ui: login uid=<uid>`
     - `ui: lockscreen shown`
5. CLI `nx-user` / `nx-session` (host tools):
   - list/add/passwd/del users
   - login/lock/unlock/switch
   - NOTE: QEMU selftests must not rely on running host tools inside QEMU
6. OS selftests (bounded):
   - `SELFTEST: oobe create+login ok`
   - `SELFTEST: session lock+unlock ok`
   - `SELFTEST: session switch ok`
   - `SELFTEST: securefs home ok` (only if SecureFS is truly enabled; otherwise explicit placeholder)
7. Docs:
   - multi-user model and session lifecycle
   - SecureFS home layout and gating
   - keystore binding model and security caveats

## Non-Goals

- Kernel changes.
- Claiming deterministic-nonce encryption schemes for production.
- Biometric auth beyond explicit stubs.

## Constraints / invariants (hard requirements)

- `/state` gating: persistence is only real when `TASK-0009` exists.
- SecureFS gating: encryption at rest is only real when `TASK-0183` is unblocked (and its red flags addressed).
- No fake success markers:
  - “securefs home ok” must validate read/write roundtrip through the mounted secure namespace and must be skipped if SecureFS is placeholder.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p accounts_identity_v1_2_host -- --nocapture` (from v1.2a)

- **Proof (QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s ./scripts/qemu-test.sh`
  - Required markers:
    - `SELFTEST: oobe create+login ok`
    - `SELFTEST: session lock+unlock ok`
    - `SELFTEST: session switch ok`
  - Optional (only if SecureFS is real):
    - `SELFTEST: securefs home ok`

## Touched paths (allowlist)

- `source/services/sessiond/` (new)
- `source/services/identityd/` (integration)
- `source/services/lockd/` (integration)
- `source/services/securefsd/` (integration use only)
- `source/services/keystored/` (user-domain unlock integration)
- `userspace/apps/oobe/` + `userspace/apps/login/` + `userspace/apps/accounts/` (wiring)
- `tools/nx-user/` (or `nx session` subcommands)
- `source/apps/selftest-client/`
- `schemas/accounts_v1_2.schema.json`
- `docs/accounts/` + `docs/tools/nx-user.md` + `docs/dev/ui/testing.md`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. sessiond OS wiring + markers
2. SecureFS home mount model + gating
3. keystore per-user unlock integration + gating
4. Greeter/OOBE/lock wiring + selftests + docs + postflight wrapper (delegating)

## Acceptance criteria (behavioral)

- In QEMU, OOBE/login/lock/unlock/switch flows are proven deterministically; secure home is proven only when SecureFS is unblocked and real.
