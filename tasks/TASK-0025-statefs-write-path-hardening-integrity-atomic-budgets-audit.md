---
title: TASK-0025 StateFS write-path hardening: authenticity envelopes + anti-rollback + budgets (rebased 2026-07-15 onto shipped statefs v1)
status: Draft
owner: @runtime
created: 2025-12-22
depends-on:
  - TASK-0006
  - TASK-0008
  - TASK-0009
  - TASK-0019
follow-up-tasks:
  - TASK-0027
  - TASK-0132
  - TASK-0133
  - TASK-0289
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Shipped substrate (v1, Complete): docs/rfcs/RFC-0018-statefs-journal-format-v1.md
  - Architecture split: docs/adr/0043-user-data-in-dedicated-cow-fs-statefs-stays-service-kv.md
  - Current-state doc: docs/storage/statefs.md
  - Track: tasks/TRACK-STASH-USER-DATA-FS.md
  - Testing contract: scripts/qemu-test.sh
---

## Context (rebased 2026-07-15)

This task was drafted 2025-12-22 when statefs did not exist. **statefs v1 has since shipped and is
proven** (TASK-0009 Done; RFC-0018 Complete; ADR-0023). The original scope is therefore partly
delivered by v1 itself. What v1 already provides — do NOT re-implement:

- **Integrity (per record)**: CRC32-C over every journal record, bounded deterministic replay that
  stops at the first corrupt/truncated record (`userspace/statefs/src/lib.rs` — engine, ~1630 LOC;
  `MAX_REPLAY_RECORDS`, truncated-tail tests).
- **Single-op atomicity**: each Put/Delete is one journal append; a torn append is discarded by
  replay. (Multi-op/2PC atomicity is TASK-0026, not here.)
- **Size budgets**: `MAX_KEY_LEN = 255`, `MAX_VALUE_SIZE = 64 KiB`, effective ~8 KiB per value over
  IPC (frame cap enforced in `source/services/statefsd/src/os_lite.rs`); deterministic
  `VALUE_TOO_LARGE`/`KEY_TOO_LONG`/`INVALID_KEY` statuses.
- **Policy + audit**: per-op caps `statefs.{read,write,keystore,boot}` via policyd deny-by-default
  (RFC-0066 chain), denial audit records to logd (`os_lite.rs` `append_logd_audit`).
- **Key hygiene**: `/state/` root enforced; `..`/`.` rejected.
- Real consumers in production use: keystored (`/state/keystore/device.signing`), updated
  (`/state/boot/bootctl.v1`), settingsd (`/state/settingsd/prefs`).

**What v1 does NOT provide** — the residual scope of this task:

1. **Authenticity**: CRC32-C detects corruption, not tampering. An attacker who can write the
   medium can forge records that replay cleanly. RFC-0018 documents this gap explicitly.
2. **Anti-rollback**: replay is last-writer-wins over whatever the journal contains; truncating the
   journal silently rolls state back. Nothing detects it.
3. **Latency budgets**: no per-op deadline/warn accounting exists.
4. **Client-crate debt**: settingsd speaks a hand-rolled copy of the SF wire protocol
   (`source/services/settingsd/src/statefs_client.rs`) instead of `statefs::client` — a drift bomb
   for any envelope/wire evolution.

Per-subject **quotas** stay in TASK-0133. User-data encryption is **not** statefs business
(ADR-0043 / RFC-0071); statefs record encryption for its own values is TASK-0027.

## Goal

Prove, deterministically:

- Host: authenticity envelopes + anti-rollback counters behave correctly, including negative cases
  (forged value, rolled-back journal) with stable error mapping and audit emission.
- OS/QEMU: selftest markers for authenticated put/verify and tamper/rollback denial, without fake
  success.

## Non-Goals

- Multi-op transactions/2PC, compaction, fsck (TASK-0026).
- Encryption of values (TASK-0027) or user data (RFC-0071).
- Quota accounting (TASK-0133).
- Any RFC-0018 journal-byte change: the envelope lives **inside the value payload**; journal record
  layout stays byte-identical (v1 replay still works).
- Kernel changes.

## Constraints / invariants (hard requirements)

- Kernel untouched; statefsd stays the sole `/state` authority.
- Envelope metadata bounded (fixed caps); parsing bounded; no `unwrap/expect`; no blanket
  `allow(dead_code)`.
- No fake markers: emit success only after verify actually ran against replayed bytes.
- Envelope is opt-in per key-prefix class so existing consumers keep working during migration;
  fail-closed once a prefix is declared envelope-mandatory.
- Key material for HMAC comes from keystored via HKDF with a labeled context
  (`"statefs.envelope.v1"`); never a signing key used raw. **Chicken-egg rule**: the keystored
  device-key record itself (`/state/keystore/*`) cannot depend on keystored-derived MACs for its
  own bootstrap read — boot-critical prefixes use envelope integrity + anti-rollback counter, with
  authenticity provided by the boot chain (documented, not faked).

## Red flags / decision points

- **YELLOW (counter storage)**: the anti-rollback high-water counter must survive journal
  truncation — v1 stores it as a monotonic `seq` in each envelope + max-seen check at replay
  (detects rollback of *individual keys*), full-journal rollback detection needs an out-of-band
  anchor (RFC-0071-era or boot-chain TASK-0289) — v1 documents the boundary honestly.
- **YELLOW (migration)**: updated/keystored writes migrate to `put_authenticated` behind their
  existing markers; settingsd first migrates onto `statefs::client` (debt payoff), then envelopes.

## Contract sources (single source of truth)

- Journal substrate: RFC-0018 (Complete; unchanged).
- Envelope value-format: documented in `docs/storage/statefs.md` §"Authenticity envelope v1"
  (this task keeps that section normative; no new RFC needed — value-internal format, journal
  contract untouched).
- Policy/audit chain: RFC-0015 / RFC-0066 as wired today.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- `cargo test -p statefs` extended:
  - wrap + put + restart/replay → verify ok, same bytes
  - forged value (bit-flip in payload, valid CRC re-computed) → `EINTEGRITY`-class status
  - stale `seq` re-applied (rollback of a key) → rejected + status stable
  - oversize envelope metadata → deterministic reject
  - latency budget exceeded (simulated slow sink) → warn accounting visible in test hook
- settingsd uses `statefs::client` (hand-rolled wire deleted); its prefs contract test stays green.

### Proof (OS / QEMU)

- `statefsd: write hardening on (auth-envelope)`
- `SELFTEST: statefs auth put ok`
- `SELFTEST: statefs tamper deny ok`
- `SELFTEST: statefs rollback deny ok`

## Touched paths (allowlist)

- `userspace/statefs/` (envelope module + client `put_authenticated`)
- `source/services/statefsd/` (verify-on-put for envelope-mandatory prefixes, budgets)
- `source/services/settingsd/` (migrate to statefs client crate)
- `source/services/keystored/`, `source/services/updated/` (adopt envelopes; gated)
- `source/apps/selftest-client/` (markers)
- `docs/storage/statefs.md`, `docs/testing/index.md`, `scripts/qemu-test.sh`

## Plan (small PRs)

1. Envelope v1 in `userspace/statefs` (host-first): `{ver, alg, seq, hmac?, meta{subject,purpose,ts}}`,
   CBOR-or-fixed-struct encoding, strict caps; wrap/verify + replay-time max-seq tracking.
2. statefsd: per-prefix envelope policy (off / integrity / authenticated), budgets + warn path.
3. settingsd client-crate migration (independent, lands first — pure debt payoff).
4. keystored/updated adoption + selftest markers.
