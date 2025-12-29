---
title: TASK-0183 Encryption-at-Rest v1b (OS/QEMU): securefsd overlay (state:/secure) + unlock/lock/change + migration + SystemUI unlock sheet + nx-secure + policy/quotas + selftests/docs
status: Draft
owner: @security
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Crypto/KDF substrate (host-first): tasks/TASK-0182-encryption-at-rest-v1a-host-secure-keys-io-format-tests.md
  - Keystore seal/unseal OS wiring: tasks/TASK-0160-identity-keystore-v1_1-os-attestd-trust-unification-selftests.md
  - Persistence substrate (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Quotas model: tasks/TASK-0133-statefs-quotas-v1-accounting-enforcement.md
  - Backup/Restore integration (exclusions): tasks/TASK-0162-backup-restore-v1b-os-backupd-settings-cli-selftests-docs.md
  - Policy capability matrix: tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

This task exposes an app-facing encrypted overlay `state:/secure/**` via a `securefsd` service:

- content encryption only (filenames plaintext in v1),
- unlock/lock/change passphrase APIs,
- migration helper to move selected paths into SecureFS,
- SystemUI unlock sheet and Settings hooks,
- deterministic host+OS proofs with strict “no fake success” markers.

This must be offline and kernel-untouched. It is OS-gated on `/state` and keystore seal/unseal.

## Goal

Deliver:

1. `securefsd` service (`source/services/securefsd/`) with Cap’n Proto IDL:
   - `status`, `unlock`, `lock`, `changePass`, `migratePath`, `listSecure`, `scrubKeys`
   - overlay layout:
     - `state:/secure/` (virtual mount path)
     - `state:/secure_blob/` (encrypted file backing store)
     - `state:/secure_meta/` (kdf params, wrapped mek, bookkeeping)
   - atomic writes via temp + rename
   - tamper detection surfaced as stable errors + markers
   - crypto contract (MUST match `TASK-0182`):
     - per-file `file_id` (128-bit) and per-file `nonce_prefix`
     - per-file subkey derived from MEK + `file_id` (HKDF)
     - chunk nonce = `nonce_prefix || chunk_index_le` (64-bit counter)
     - AAD binds to `file_id` + header metadata + chunk index (NOT path)
   - markers:
     - `securefsd: ready`
     - `securefs: unlock ok`
     - `securefs: lock`
     - `securefs: migrate done files=<n>`
     - `securefs: tamper detect`
2. Policy + quotas:
   - caps:
     - `securefs.unlock`, `securefs.lock`, `securefs.pass.change`, `securefs.migrate`, `securefs.list`
   - apps do not get key APIs; apps only see `state:/secure/...` via VFS namespaces (future sandbox tasks)
   - quota enforcement (soft/hard) on `secure_blob` + `secure_meta` (gated on quota substrate)
3. SystemUI:
   - unlock sheet shown at boot if configured and SecureFS is locked
   - Settings entry under Security & Privacy for passphrase change / lock now / migrate
   - deterministic lockout after N failures with exponential backoff (injectable clock for tests)
4. Migration:
   - recursive copy from allowlisted source prefixes (skip caches)
   - idempotent when destination exists
   - optional auto-migrate list from config
5. CLI `nx-secure`:
   - status/set-pass/unlock/lock/migrate/ls
   - NOTE: do not rely on running host CLIs inside QEMU selftests
6. OS selftests (bounded):
   - `SELFTEST: securefs set-pass ok`
   - `SELFTEST: securefs io ok`
   - `SELFTEST: securefs lock deny ok`
   - `SELFTEST: securefs unlock ok`
   - `SELFTEST: securefs migrate ok`
7. Docs:
   - threat model + limitations (filenames plaintext, offline)
   - key hierarchy + entropy constraints
   - migration and backup exclusions

## Non-Goals

- Kernel changes.
- Filename encryption (v2+).
- Full per-app sandbox enforcement (separate sandboxing tasks).
- Claiming production security without entropy.

## Constraints / invariants (hard requirements)

- No passphrases in logs; redaction enforced.
- No fake success markers.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **RED (/state required)**:
  - SecureFS persistence and migration require `TASK-0009`.

- **RED (keystored seal/unseal required)**:
  - MEK wrapping depends on keystored v1.1 (`TASK-0159/0160`). Without it, SecureFS must be `stub/placeholder` and must not claim encryption.

- **RED (entropy / nonce generation)**:
  - If OS RNG story is not solved, OS must not claim “secure”.
  - SecureFS must either:
    - refuse to enable encryption-at-rest (hard fail / “disabled” status), or
    - run in an explicit `stub/placeholder` mode with markers and docs stating it is insecure.
  - Deterministic AEAD nonce modes are forbidden in production; any seeded/test RNG mode is test-only and must be labeled insecure.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p encryption_at_rest_v1_host -- --nocapture` (from v1a)

- **Proof (QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=195s ./scripts/qemu-test.sh`
  - Required markers:
    - `securefsd: ready`
    - `SELFTEST: securefs set-pass ok`
    - `SELFTEST: securefs io ok`
    - `SELFTEST: securefs lock deny ok`
    - `SELFTEST: securefs unlock ok`
    - `SELFTEST: securefs migrate ok`

## Touched paths (allowlist)

- `source/services/securefsd/` (new)
- `source/services/keystored/` (integration use only)
- `source/apps/selftest-client/`
- `userspace/systemui/` (unlock overlay + settings wiring)
- `tools/nx-secure/` (new; host/dev tool)
- `schemas/securefs.schema.json` (new)
- `docs/securefs/` + backup doc note
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. securefsd IDL + service skeleton + markers
2. integrate secure-keys/secure-io + keystored seal/unseal + redaction
3. migration + quotas + config schema
4. SystemUI unlock/settings + selftests + docs + marker contract update

## Acceptance criteria (behavioral)

- In QEMU (when unblocked), securefs can be locked/unlocked and IO roundtrips with tamper detection, and migration emits deterministic markers.

Follow-up:

- Accounts/Identity v1.2 mounts per-user homes under SecureFS (`state:/secure/home/<uid>/`) after login. Tracked as `TASK-0224`.
