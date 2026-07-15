---
title: TASK-0027 StateFS v2b: record encryption for statefs values (rescoped 2026-07-15 — user-data encryption moved to RFC-0071/nxfs)
status: Draft
owner: @runtime
created: 2025-12-22
depends-on:
  - TASK-0006
  - TASK-0008B
  - TASK-0009
  - TASK-0025
  - TASK-0026
follow-up-tasks: []
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Shipped substrate (v1, Complete): docs/rfcs/RFC-0018-statefs-journal-format-v1.md
  - Key hierarchy + user-data encryption (authoritative): docs/rfcs/RFC-0071-nxfs-user-data-filesystem-contract.md
  - Architecture split: docs/adr/0043-user-data-in-dedicated-cow-fs-statefs-stays-service-kv.md
  - Device keys / entropy: tasks/TASK-0008B-device-identity-keys-v1-virtio-rng-rngd-keystored-keygen.md
  - Track: tasks/TRACK-STASH-USER-DATA-FS.md
  - Testing contract: scripts/qemu-test.sh
---

## Context (rescoped 2026-07-15)

Originally this task carried the whole "encryption at rest" ambition. That has been split by
ADR-0043 / RFC-0071:

- **User-data encryption (files under `/data`) is NOT this task.** It is an nxfs volume/file
  encryption class — RFC-0071 Phase 4 owns the contract (AEAD, key hierarchy, honest
  no-sealed-storage limitation). The old "securefsd" overlay tasks (TASK-0182/0183) are superseded
  by the same RFC.
- **This task keeps the narrow remainder**: optional AEAD encryption of **statefs record values**
  (service state under `/state/`), reusing the RFC-0071 key-hierarchy contract
  (keystored material → HKDF, labeled context) so the platform has ONE key-derivation discipline.

Repo reality (2026-07-15): statefs v1 shipped (TASK-0009 Done) — plaintext values, CRC32-C
integrity only. keystored exists and persists its ed25519 device key **in statefs**
(`/state/keystore/device.signing`); rngd exists (entropy, no persistence). `chacha20poly1305` is
present in `Cargo.lock` only transitively (dsoftbus Noise) — no at-rest wiring, no `hkdf`/`zeroize`
in-tree yet.

**Chicken-egg (normative for this task)**: records keystored needs in order to start —
`/state/keystore/*`, `/state/boot/*` — can never be encrypted under keystored-derived keys. These
boot-critical prefixes stay plaintext-but-authenticated (TASK-0025 envelopes). Encryption applies
to non-boot-critical prefixes (e.g. `/state/settingsd/*`, `/state/app/*`) — per-prefix class,
default **off**.

## Goal

Provide an opt-in `STATEFS_ENCRYPTION=on` mode that:

- encrypts value payloads of enrolled prefixes with AEAD (XChaCha20-Poly1305),
- detects tampering deterministically (`EINTEGRITY`-class status),
- preserves v2a crash-atomicity (only committed txns visible; decrypt failure discards the txn),
- keeps compaction + fsck working (snapshot values remain decryptable; fsck reports decrypt
  failures, never "fixes" ciphertext),
- is testable deterministically on host and proven in OS/QEMU.

## Non-Goals

- User-data/file encryption (RFC-0071 Phase 4).
- Metadata/key-name encryption (paths stay plaintext in v2b; documented explicitly).
- Key rotation (follow-up once RFC-0071 P4 fixes the rekey model).
- Kernel changes.

## Constraints / invariants (hard requirements)

- Kernel untouched. Default stays green: encryption OFF by default; `statefsd: encryption off`
  marker when disabled.
- Key derivation per RFC-0071 contract: keystored material → HKDF with label
  `"statefs.record.v1.<prefix-class>"`; never a signing key used raw as an AEAD key.
- Nonce construction deterministic and never-reusing: bound to `(txn_id, chunk_idx)` from the v2a
  record framing (this is why TASK-0026 is a hard dependency).
- AAD binds record header fields (`txn_id`, key hash, payload length) — ciphertext is tied to its
  record.
- Recovery idempotent under decrypt failures; bounded memory/parsing; no `unwrap/expect`.
- **RED (entropy honesty)**: if the OS build cannot provide secure entropy for salts, do not claim
  secure encryption — keep the mode unavailable in OS and say so (`statefsd: encryption unavailable
  (entropy)`), host tooling only.

## Contract sources (single source of truth)

- Key hierarchy + AEAD discipline: RFC-0071 (Security considerations + encryption-class contract).
- Record framing hooks: TASK-0026's journal v2 (`docs/storage/statefs.md` §"Journal v2 (2PC)").
- Superblock/enablement flags: `docs/storage/statefs.md` §"Record encryption (v2b)" (kept normative
  by this task): `enc_mode`, `key_descriptor` (opaque, e.g. "device-key-v1"), salt.

## Stop conditions (Definition of Done)

### Proof (Host) — required

Tests (crate-local in `userspace/statefs` + `tools/fsck-statefs`):

- encryption on: write/read roundtrip for an enrolled prefix; replay works
- boot-critical prefix enrollment attempt → rejected deterministically (chicken-egg guard)
- tamper ciphertext: replay rejects with `EINTEGRITY`-class status and discards the txn
- compaction with encryption: snapshot values remain decryptable
- fsck: reports decrypt failures clearly; `--repair` removes unrecoverable txns from the active
  set, never rewrites ciphertext
- nonce-uniqueness property test over txn/chunk space

### Proof (OS / QEMU)

When enabled and entropy is available:

- `statefsd: encryption on (xchacha20poly1305)`
- `SELFTEST: statefs enc roundtrip ok`
- `SELFTEST: statefs enc tamper deny ok`

Otherwise:

- `statefsd: encryption off` (or `… unavailable (entropy)`)

## Touched paths (allowlist)

- `userspace/statefs/` (encrypt/decrypt payload path on v2a records)
- `source/services/statefsd/` (enablement, prefix classes; gated)
- `source/services/keystored/` (expose HKDF-derived AEAD key handle; gated)
- `tools/fsck-statefs/` (decrypt-aware validation)
- `docs/storage/statefs.md`, `scripts/qemu-test.sh` (markers)

## Docs (English)

- Document explicitly in `docs/storage/statefs.md`:
  - what is encrypted (enrolled-prefix values) and what is plaintext (keys/paths, metadata,
    boot-critical prefixes),
  - threat model + entropy requirements + the no-sealed-storage limitation (same honesty rule as
    RFC-0071),
  - enablement flags and expected markers.
