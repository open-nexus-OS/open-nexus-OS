---
title: TASK-0027 StateFS v2b: record encryption for statefs values (rescoped 2026-07-15 — user-data encryption moved to RFC-0071/nxfs)
status: Done
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
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
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

**Verified still open (2026-08-14, storage reconciliation):** repo reality unchanged — values
plaintext, no AEAD/HKDF wiring in-tree; hard deps TASK-0025 and TASK-0026 are still open. The
nxfs-side sibling (user-data encryption classes) is now explicitly seeded as TASK-0320; the two
share the RFC-0071 key-derivation discipline but remain separate stores.

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

## Progress — 2026-08-18: implemented end to end (host green; QEMU double boot running)

**Design (architecture-review, three lenses):** engine-owned sealing — `statefs::enc` (new
module): sealed value `NXR1 v1` (36-byte overhead: header `magic|ver|class|reserved|txn_id
u64|chunk_idx u32` + Poly1305 tag), nonce = `salt(12) || txn_id || chunk_idx` (deterministic,
no getrandom — RFC-0009; plain puts consume ids from the txn counter, replay re-seeds the
counter above every id in PREPARE records AND sealed headers, so nonces never repeat across
reopen/compaction), AAD binds header + full key path (splice-proof). Values stay sealed in
kv and through compaction (snapshots copy ciphertext — compaction/readback-verify untouched);
`get` opens on demand; replay AEAD-verifies when the context is installed (tampered plain put
skipped + counted, tampered txn chunk poisons its whole txn via the new `Replayer::poison`).
Enrolled txn values are single-chunk (sealing forbids concatenation; chunk_idx = txn entry
slot). Keys: HKDF over the deterministic Ed25519 device-seed signature of
`statefs.record.v1.<class>` — statefsd-local, NO keystored oracle (`device_sign_allowed`
untouched). **Enablement**: admin meta record `/state/statefsd/enc.v1` (NXEM: salt, CRC,
zero-salt rejected) behind a NEW `statefs.admin` cap-table row (plain `statefs.write` cannot
toggle; granted to selftest-client in policies/base.toml); salt is per-store forever;
statefsd enables at mount/virtio-upgrade/device-key-write/meta-write and only after an
in-process seal/open/tamper self-check gates `statefsd: encryption on (xchacha20poly1305)`.
Deviation from the ledger's OS DoD, documented: `SELFTEST: statefs enc tamper deny ok` is not
honestly producible over the wire (the wire carries plaintext; the adversary is the disk) —
tamper negatives are host proofs (replay/get/fsck + TamperDevice) and the on-marker is gated
by statefsd's in-process tamper self-check; the OS selftest proves enable + roundtrip +
AEAD-verified replay via Sync+Reopen (`SELFTEST: statefs enc roundtrip ok`).

**Touched:** `userspace/statefs/{enc.rs (new), lib.rs, journal_v2.rs, record.rs, fsck.rs}`
(encode helpers moved to record.rs for the ratchet — journal_v2 587→~560), statefsd
`{enc_svc.rs (new, cfg-free), enc_os.rs (new), hardening_os.rs (new — envelope glue moved
out of os_lite.rs, 628→580), os_lite.rs hooks, txn.rs cap mirror}`, `tools/fsck-statefs`
(`--enc-key-hex/--enc-class/--enc-salt-hex`), selftest `statefs_enc.rs` (salt from rngd,
idempotent meta put, roundtrip), policies/base.toml (+`statefs.admin`), qemu-test.sh
(markers + 0027 fake-green guard: on-marker ↔ roundtrip coupled both ways, FAIL signatures
fatal), statefs.md §v2b normative flesh-out. Crate: `chacha20poly1305` no-default-features
+alloc (`just dep-gate` PASS).

**Host proof:** enc unit matrix (roundtrip, per-byte tamper, cross-key splice, enrollment
chicken-egg incl. parent-prefix denial, meta codec, label bounds, domain separation);
engine tests (sealed-at-rest — plaintext never on the medium, tamper→replay skip + get
EINTEGRITY, tampered txn chunk → both-or-neither discard, compaction stays decryptable,
nonce-id monotonicity across reopen, single-chunk rule, legacy-plaintext migration);
statefsd `tests/enc_contract.rs` (seed-bound determinism, self-check,
`test_reject_enc_meta_write_without_admin_cap`, enrolled-table invariants, meta gate);
fsck (sealed counted keyless, keyed verify, `test_reject_fsck_reports_tampered_sealed_value`
— report-only, exit ≥1, never rewrites). All statefs/statefsd/fsck suites green; clippy,
diag-os, dep-gate PASS.

**Real bug found by the double-boot ladder (fixed in keystored):** keystored's initial store
load races statefsd's mem→virtio upgrade; on a preserved image it believed "no key" and
REGENERATED the device key at the keygen op — forking the device identity and orphaning
every envelope MAC'd under boot 1's key (the earlier "green" keep-blk run only passed
because the probes' time-based fallback seq happened to exceed the tracker high-water).
Fix: the keygen op now re-reads the persisted record (`store.reload_device_key()`) before
ever generating — `keystored: device key reloaded (pre-keygen)` + KEY_EXISTS on boot 2.

## Closure — 2026-08-18: Done

Final double-boot sequence GREEN (both runs exit 0, all 0025/0026 markers intact):

- boot 1, fresh image (headless--2026-08-18T15-19-56): `statefsd: encryption off` (mount) →
  device keygen → selftest enables (rngd salt → admin meta put) →
  `statefsd: encryption on (xchacha20poly1305)` (post self-check) →
  `SELFTEST: statefs enc roundtrip ok` (put/get equality before AND after Sync+Reopen =
  AEAD-verified replay in-VM), `cold-boot seeded`.
- boot 2, preserved image `NEXUS_KEEP_BLK=1 REQUIRE_STATEFS_COLD_BOOT=1`
  (headless--2026-08-18T15-21-34): `keystored: device key reloaded (pre-keygen)` (device
  identity stable across cold boots — the keystored fix), encryption on at the virtio
  upgrade (meta + seed persisted), roundtrip ok against the PRESERVED sealed store,
  `SELFTEST: statefs auth put ok` (0025 hardening green on the preserved journal),
  `SELFTEST: statefs cold-boot persist ok`.

DoD deltas, both documented above: the OS tamper negative lives in statefsd's marker-gating
self-check + host proofs (wire carries plaintext by design); `statefsd: encryption
unavailable (entropy)` became `unavailable (keys)` — statefsd itself needs no entropy
(deterministic nonces; the salt's entropy provenance is the enabler's, and the selftest
only enables after a successful rngd read, which is the RED rule enforced end-to-end).
Follow-ups unchanged (non-goals): key rotation, path/metadata encryption, `/data`
encryption classes (TASK-0320 / RFC-0071 P4).

## Docs (English)

- Document explicitly in `docs/storage/statefs.md`:
  - what is encrypted (enrolled-prefix values) and what is plaintext (keys/paths, metadata,
    boot-critical prefixes),
  - threat model + entropy requirements + the no-sealed-storage limitation (same honesty rule as
    RFC-0071),
  - enablement flags and expected markers.
