---
title: TASK-0026 StateFS v2a: 2PC crash-atomicity + bounded compaction + fsck tool (rebased 2026-07-15 onto shipped statefs v1)
status: Draft
owner: @runtime
created: 2025-12-22
depends-on:
  - TASK-0009
follow-up-tasks:
  - TASK-0027
  - TASK-0134
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Shipped substrate (v1, Complete): docs/rfcs/RFC-0018-statefs-journal-format-v1.md
  - Architecture split: docs/adr/0043-user-data-in-dedicated-cow-fs-statefs-stays-service-kv.md
  - Block layer / cold-boot proofs: docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md
  - Current-state doc: docs/storage/statefs.md
  - Track: tasks/TRACK-STASH-USER-DATA-FS.md
  - Testing contract: scripts/qemu-test.sh
---

## Context (rebased 2026-07-15)

Drafted 2025-12-22 when statefs was "still tasks, not shipped code". **statefs v1 has shipped**
(TASK-0009 Done; RFC-0018 Complete): append-only `NXSF` journal with CRC32-C records, ops
Put/Get/Delete/List/Sync/Reopen, bounded deterministic replay, real consumers (keystored, updated,
settingsd). The scope of THIS task is still **fully open** — v1 has:

- **No multi-op atomicity.** Each Put/Delete commits alone; there is no way to update two keys
  atomically (e.g. bootctl slot + tries counter).
- **No compaction.** The `Checkpoint` opcode (0x03) is parsed and **deliberately no-op**
  (`userspace/statefs/src/lib.rs`, replay match arm) — reserved for exactly this task. The journal
  grows forever; replay time grows with it (bounded only by `MAX_REPLAY_RECORDS = 100_000`,
  after which the store fails to open).
- **No offline tooling.** No fsck; a damaged journal can only be truncated-at-first-error by
  replay.

This is the same 2PC/compaction/fsck discipline nxfs P1 needs (RFC-0071); patterns and test
harnesses built here are explicitly meant to be reused there (ADR-0043 consequence).

## Goal

Prove deterministically (host tests) that:

- only committed transactions become visible after replay,
- prepared-but-not-committed transactions are discarded,
- compaction produces a minimal snapshot and a clean journal with bounded work per cycle,
- `fsck-statefs` detects and (optionally) repairs orphaned transactions with stable exit codes.

Then prove it in OS/QEMU including a **cold-boot** persistence cycle via `NEXUS_KEEP_BLK=1`
(ADR-0044) — the current "persist ok" evidence only ever proved soft-reboot replay (the launcher
recreates `build/blk.img` every boot).

## Non-Goals

- Encryption-at-rest (TASK-0027 for statefs records; RFC-0071 for user data).
- Named snapshots / read-only snapshot mounts (TASK-0134 remainder).
- Authenticity envelopes (TASK-0025; independent — envelopes live inside values, 2PC lives in
  record framing; the two compose).
- Full filesystem semantics (ADR-0043: that is nxfs).
- Kernel changes.

## Constraints / invariants (hard requirements)

- Kernel untouched.
- **Backward compatible**: v1 journals replay unchanged; v2 records are new opcodes appended to the
  existing framing (`NXSF | op | keylen | vallen | key | value | crc32c`); first compaction writes
  a v2-generation journal. RFC-0018 stays Complete — the v2 record set is documented in
  `docs/storage/statefs.md` §"Journal v2 (2PC)" as this task's normative contract.
- **Bounded everything**: txn-in-flight buffers capped (count + bytes), chunk size capped, replay
  bounded, compaction work per cycle bounded.
- No `unwrap/expect`; no fake markers (compaction marker only after the rotated journal re-replays
  clean).

## Red flags / decision points

- **YELLOW (delete semantics)**: kept from the original draft — `DELETE` is its own committed
  record (immediate during replay), not transactional, in v2a.
- **YELLOW (Sync op overlap)**: v1 already has a `Sync` protocol op; the v2 `SYNC` journal record
  is distinct (durability barrier in the log). Name them apart in code.
- **RED (consumer safety)**: keystored/updated must keep booting against a v2-compacted journal —
  their contract tests are part of this task's gate.

## Contract sources (single source of truth)

- v1 substrate: RFC-0018 (Complete, unchanged for v1 records).
- v2 record set + compaction + fsck semantics: `docs/storage/statefs.md` §"Journal v2 (2PC)"
  (kept normative by this task).
- QEMU marker contract: `scripts/qemu-test.sh`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`cargo test -p statefs` (+ `cargo test -p fsck-statefs`):

- happy path: `PREPARE + PAYLOAD + COMMIT` → visible after replay
- crash simulation: `PREPARE + partial PAYLOAD` (no COMMIT) → not visible after replay
- multi-key txn: both-or-neither across restart at every record boundary
- idempotence: replay same journal twice → same state
- v1 journal → v2 upgrade path: replay v1, compact, resulting journal is v2 + state identical
- compaction: threshold → snapshot+rotate; state intact; bounded cycle work observable
- fsck: detect orphaned txns; `--repair` converts orphans to ABORT; exit codes stable
  (0 ok, 1 repaired, 2 unrecoverable)

### Proof (OS / QEMU)

- `statefsd: journal v2 mounted (2PC)`
- `SELFTEST: statefs v2 crash-atomic ok`
- `SELFTEST: statefs v2 compact ok`
- `statefsd: compaction done (gen=<n>, entries=<m>)`
- Cold boot (gated on ADR-0044 keep-blk): `SELFTEST: statefs cold-boot persist ok`

## Touched paths (allowlist)

- `userspace/statefs/` (journal v2 records + 2PC replay + compaction)
- `source/services/statefsd/` (txn ops, compaction trigger)
- `tools/fsck-statefs/` (new host tool)
- `source/apps/selftest-client/` (markers)
- `docs/storage/statefs.md`, `docs/testing/README.md`, `scripts/qemu-test.sh`

## Plan (small PRs)

1. **Journal v2 record set + replay (2PC)** — opcodes `PREPARE{txn_id,…}`, `PAYLOAD{txn_id,chunk}`,
   `COMMIT{txn_id}`, `ABORT{txn_id}`, `SYNC{}` on the existing framing; replay applies
   committed-only; v1 records keep replaying as-is.
2. **Compaction (snapshot + rotate)** — reuse the reserved `Checkpoint` opcode as the snapshot
   boundary; threshold configurable (ratio or bytes); bounded work per cycle.
3. **fsck-statefs (host)** — offline replay/validate/repair, deterministic output + exit codes.
4. **OS selftest + cold-boot gate** — Reopen-based soft-reboot proof stays; add keep-blk cold-boot
   cycle once ADR-0044 lands (TASK-0293 wiring).
