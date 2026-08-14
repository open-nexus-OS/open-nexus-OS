---
title: TASK-0320 nxfs Phase 4: encryption classes (per-extent XChaCha20-Poly1305, keystored HKDF hierarchy) (RFC-0071 P4)
status: Draft
owner: @runtime
created: 2026-08-14
depends-on:
  - TASK-0319
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Contract (Phase 4, fixed since 2026-07-15): docs/rfcs/RFC-0071-nxfs-user-data-filesystem-contract.md
  - Key material: docs/rfcs/RFC-0016-device-identity-keys-v1.md (keystored)
  - Supersedes (UX pieces absorbed): tasks/TASK-0182 / tasks/TASK-0183 (securefsd overlay — already Superseded by RFC-0071)
  - statefs sibling (record encryption, separate store): tasks/TASK-0027-statefs-v2b-encryption-at-rest.md
  - Ladder: tasks/TRACK-STASH-USER-DATA-FS.md (replaces milestone 7 "seed-when-ready")
---

## Context

RFC-0071 Phase 4 fixes the encryption contract completely — this task executes it. Today
`enc_mode` is a reserved superblock byte hardcoded to 0; no AEAD/HKDF wiring exists anywhere in
the tree. The format carries the per-file class field since TASK-0316 (value = inherit), and the
nonce input (`txn_id`, extent index, generation counters) exists since v1 — deliberate contract
choices so this phase needs **no format break**.

## Goal

- **Classes `None` / `Device`** live (`User` stays reserved): per-volume default + per-file
  override via the class field; class = policy at write time, inherited by new objects.
- **AEAD**: XChaCha20-Poly1305 **per extent**; nonce deterministically from
  `(volume_key, object_id, extent_index, txn_id)` — never reused (property test over the space);
  AAD binds `(container_uuid, volume_id, object_id, logical_offset, payload_len)`.
- **Key hierarchy**: keystored device material → HKDF label
  `nxfs.volume.<container-uuid>.<volume-id>` → volume key; per-file wrapped keys reserved.
  Signing keys are never used as AEAD keys (same rule as TASK-0025/0027).
- Snapshots/clones remain consistent under encryption (frozen roots decrypt with the volume
  key); fsck reports decrypt failures and never "repairs" ciphertext.
- Tamper = deterministic `EINTEGRITY`-class reject, fail-closed.

## Non-Goals

- `User`-class keys / per-user unlock UX (needs the accounts/lock spine — TASK-0223/0224 line).
- Key rotation / rekey (follow-up once the wrapped-key model is live — same honesty as
  TASK-0027).
- statefs record encryption (TASK-0027 owns `/state`; shared discipline, separate store).
- Metadata/name encryption (paths stay plaintext in P4 — documented, per contract).

## Constraints / invariants (hard requirements)

- **Entropy honesty (RED, same rule as TASK-0027)**: if the OS build cannot provide secure
  entropy for salts, the mode stays unavailable with `nxfsd: encryption unavailable (entropy)` —
  never a fake `encryption on`.
- **No sealed storage on QEMU**: the `Device` class protects against medium-only theft, nothing
  stronger — markers and docs say exactly that (RFC-0071 honesty limit).
- Default OFF; opt-in per volume; default boot stays green with `nxfsd: encryption off`.
- Bounded parsing, no `unwrap`/`expect` on ciphertext/headers; zeroize key material.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- Roundtrip per class (write→read, replay, snapshot mount); nonce-uniqueness property test;
  tamper reject (`test_reject_tampered_extent`); wrong-key reject; fsck decrypt-failure
  reporting; crash-injection green with encryption on.

### Proof (OS / QEMU) — required

- `nxfsd: encryption on (device-class)` (opt-in lane) + `SELFTEST: nxfs enc roundtrip ok` +
  `SELFTEST: nxfs enc tamper deny ok`; default lane shows `nxfsd: encryption off` and stays
  green.

## Touched paths (allowlist)

- `userspace/nxfs/`, `tools/fsck-nxfs/`, `source/services/nxfsd/`,
  `source/services/keystored/` (HKDF derivation surface)
- `source/apps/selftest-client/`, `scripts/qemu-test.sh`
- `docs/rfcs/RFC-0071-…` (P4 status; approval-gated), `docs/storage/nxfs.md`,
  `docs/standards/SECURITY_STANDARDS.md` cross-ref if needed
