---
title: TASK-0027 StateFS v2b: optional encryption-at-rest (AEAD) via keystored (gated, default off)
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Depends-on (statefs v1): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Depends-on (statefs v2a): tasks/TASK-0026-statefs-v2a-2pc-compaction-fsck.md
  - Depends-on (device keys / entropy): tasks/TASK-0008B-device-identity-keys-v1-virtio-rng-rngd-keystored-keygen.md
  - Depends-on (audit sink): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Encryption-at-rest is a “deluxe” feature that must not compromise determinism, recovery semantics, or
boot reliability. It should be:

- **optional** and **disabled by default**,
- keyed from device identity material managed by `keystored`,
- designed so that recovery/compaction stay correct under crash and power loss.

This task builds on statefs v2a (2PC + compaction), because encryption must integrate with the v2
record format and snapshotting.

Related work (overlay encryption):

- App-facing encrypted overlays (content encryption on top of `/state`, filenames plaintext v1) are tracked separately as
  `TASK-0182`/`TASK-0183` (“securefsd”). That is not a replacement for statefs record encryption; it is a different layer.

## Goal

Provide an opt-in `STATEFS_ENCRYPTION=on` mode that:

- encrypts payload chunks with AEAD,
- detects tampering deterministically (`EINTEGRITY`),
- preserves crash-atomicity (only committed txns become visible),
- is testable deterministically on host (and later on OS once statefs exists).

## Non-Goals

- Full metadata encryption (paths remain plaintext in v2b; document explicitly).
- Key rotation (follow-up).
- Kernel changes.

## Constraints / invariants (hard requirements)

- Kernel untouched.
- Default stays green: encryption OFF by default.
- PMTU irrelevant (block storage), but record sizes are capped and deterministic.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Recovery must be idempotent even when decrypt failures occur.

## Red flags / decision points

- **RED (entropy / key availability)**:
  - If OS builds cannot provide secure entropy, we must not claim “secure encryption”.
    In that case keep encryption unavailable in OS, or only allow host tooling.
- **YELLOW (key derivation)**:
  - Do not reuse signing keys directly as AEAD keys. Use HKDF with a labeled context string.

## Design sketch

- AEAD: `XChaCha20-Poly1305` (large nonce, simple).
- Nonce derivation: `nonce = H(session_key, txn_id || chunk_idx)` or a direct 24-byte construction from `(txn_id, chunk_idx, salt)`.
  Must be deterministic and never repeat for the same key.
- Associated data (AAD): includes record header fields (`txn_id`, `path_hash`, `payload_len`) to bind ciphertext to metadata.
- Superblock stores:
  - `enc_mode` (off/on),
  - `key_descriptor` (opaque id, e.g. “device-key-v1”),
  - optional salt.

## Stop conditions (Definition of Done)

### Proof (Host) — required

Tests (`tests/statefs_v2b_crypto_host/`):

- encryption on: write/read roundtrip, replay works
- tamper ciphertext: replay rejects with `EINTEGRITY` (and discards txn)
- compaction with encryption: snapshot values remain decryptable
- fsck-statefs:
  - reports decrypt failures clearly
  - optional `--repair` removes unrecoverable txns from the active set (never “fixes” ciphertext)

### Proof (OS / QEMU) — after TASK-0009 + entropy decision

When encryption enabled and available:

- `statefsd: encryption on (xchacha20poly1305)`
- `SELFTEST: statefs v2 enc ok`

Otherwise:

- `statefsd: encryption off`

## Touched paths (allowlist)

- `source/services/statefsd/` (encrypt/decrypt payload path; gated)
- `source/services/keystored/` (expose AEAD key material or a sealed key handle; gated)
- `tools/fsck-statefs/` (decrypt-aware validation)
- `tests/` (host tests)
- `docs/storage/statefs.md`
- `scripts/qemu-test.sh` (optional markers only)

## Docs (English)

- Explicitly document:
  - what is encrypted (payload) and what is plaintext (paths/metadata),
  - threat model and entropy requirements,
  - how to enable/disable and expected markers.
