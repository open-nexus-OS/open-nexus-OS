---
title: TASK-0182 Encryption-at-Rest v1a (host-first): secure-keys (Argon2id) + secure-io (XChaCha20-Poly1305) + file format + tamper detection + deterministic tests
status: Draft
owner: @security
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Keystore seal/unseal capability (host-first): tasks/TASK-0159-identity-keystore-v1_1-host-keystored-lifecycle-nonexportable.md
  - Keystore OS wiring (/state + caps): tasks/TASK-0160-identity-keystore-v1_1-os-attestd-trust-unification-selftests.md
  - StateFS encryption-at-rest (lower layer, optional): tasks/TASK-0027-statefs-v2b-encryption-at-rest.md
  - Persistence substrate (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Quotas model (storage): tasks/TASK-0133-statefs-quotas-v1-accounting-enforcement.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We want a deterministic “encryption-at-rest” slice for user data:

- user passphrase → KDF → master key material,
- per-file AEAD encryption with tamper detection,
- atomic write semantics,
- and explicit guardrails for OS entropy limitations.

This task is host-first: it produces the cryptographic and file-format substrate that `securefsd` will use.
OS/QEMU mounting, unlock UI, migration, and policy caps are handled in `TASK-0183`.

Important distinction:

- This task is **content encryption at the file level** for an overlay store (filenames plaintext in v1).
- `TASK-0027` is **optional StateFS record encryption** at the storage layer (a separate “deluxe” feature).

## Goal

Deliver:

1. `userspace/libs/secure-keys`:
   - Argon2id KDF: passphrase + salt + params → MEK (32 bytes)
   - persist KDF metadata (salt + params) via an adapter (host tests use temp dir)
   - MEK wrap/unwrap via keystored `seal/unseal` (host tests can use a mock keystore adapter)
   - zeroization of key material in memory (`zeroize`/`secrecy`)
   - rekey model v1:
     - `changePass` re-wraps the same MEK (no mass re-encrypt in v1)
2. `userspace/libs/secure-io`:
   - per-file encryption: `XChaCha20-Poly1305`
   - deterministic streaming chunk size (64 KiB)
   - file format v1:
     - header includes: magic/version, `file_id`, `nonce_prefix`, plaintext length, and AEAD tag
   - key/nonce strategy (to avoid “deterministic nonces” and rename brittleness):
     - generate a random `file_id` (128-bit) per encrypted file (stable across renames)
     - derive a per-file subkey from MEK and `file_id` (HKDF-SHA256 or equivalent)
     - generate a random `nonce_prefix` per file
     - chunk AEAD nonce = `nonce_prefix || chunk_index_le` (prefix + 64-bit counter)
       - chunk_index starts at 0 and increments per chunk; reject overflow deterministically
       - this guarantees nonce uniqueness within a file without relying on path-derived nonces
   - AAD binding:
     - bind ciphertext to `file_id`, header metadata, and `chunk_index` (not path)
     - rename does not require re-encryption
     - swap between two encrypted files fails because subkeys differ (`file_id` differs)
   - atomic write helper:
     - temp → fsync (if available) → rename (host tests on std fs)
   - stable error mapping:
     - tamper/auth failure → `EAUTH` (or repo-standard equivalent)
3. Host tests (`tests/encryption_at_rest_v1_host/`):
   - KDF golden vector (fixed salt/params)
   - encrypt/decrypt roundtrip
   - tamper detection (flip byte → `EAUTH`)
   - change passphrase: new works, old fails
   - determinism policy tests:
     - in test-mode (seeded RNG), ciphertext bytes are stable
     - in production-mode (real RNG), ciphertext bytes are allowed to differ while decrypting correctly

## Non-Goals

- Kernel changes.
- Filename encryption (explicitly v2+).
- Claiming “secure in OS” without an entropy story.

## Constraints / invariants (hard requirements)

- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Deterministic tests: seeded RNG and injected clock only in tests.
- Bounded inputs: cap file sizes in tests and cap header fields; reject oversized metadata deterministically.

## Red flags / decision points (track explicitly)

- **RED (entropy / nonce generation in OS)**:
  - The prompt suggests random nonces. If OS builds cannot provide secure entropy, we must not claim secure encryption.
  - v1a must document a policy:
    - production mode requires a real RNG,
    - test mode uses seeded RNG and is explicitly insecure.
  - Deterministic nonce derivation for AEAD is forbidden in production; any such mode is test-only and must be clearly labeled as insecure.

- **YELLOW (error taxonomy)**:
  - Align `EAUTH`/`EINTEGRITY` with the repo’s storage error contract direction (`TASK-0132`).

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p encryption_at_rest_v1_host -- --nocapture`
  - Required tests: KDF, roundtrip, tamper, change-pass, determinism policy.

## Touched paths (allowlist)

- `userspace/libs/secure-keys/` (new)
- `userspace/libs/secure-io/` (new)
- `tests/encryption_at_rest_v1_host/` (new)
- `docs/securefs/keys.md` (or a new doc stub; detailed docs may land in v1b)

## Plan (small PRs)

1. secure-keys KDF + key-wrapping adapter + host tests
2. secure-io file format + streaming + tamper detection + host tests
3. docs: determinism and entropy guardrails

## Acceptance criteria (behavioral)

- Host tests deterministically prove KDF/AEAD/tamper/change-pass behavior with explicit “test-only RNG” guardrails.
