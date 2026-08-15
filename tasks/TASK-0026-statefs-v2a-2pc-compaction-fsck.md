---
title: TASK-0026 StateFS v2a: 2PC crash-atomicity + bounded compaction + fsck tool (rebased 2026-07-15 onto shipped statefs v1)
status: In Progress
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

**Verified still open + reuse direction reversed (2026-08-14).** The reuse ran the other way:
nxfs P1 (TASK-0292, Done) shipped the 2PC/compaction/fsck discipline first — committed-only
`TXN_BEGIN…COMMIT` replay (`userspace/nxfs/src/journal.rs`), `fsck` with stable exit codes
(`userspace/nxfs/src/fsck.rs`, `tools/fsck-nxfs/`), and the SpyDevice crash-injection harness
(`userspace/nxfs/tests/crash_injection.rs`). **Reuse those patterns here**, don't re-invent.
The cold-boot dependency is also delivered: `NEXUS_KEEP_BLK=1` landed with TASK-0293 (Done) —
the cold-boot DoD gate below is unblocked.

**Performance/robustness stakes (sharpened 2026-08-14, storage-perf sweep):** this is not
polish — it defuses a boot time bomb and the statefs share of "storage is slow":

- The journal grows **unbounded** (`append_record` only ever advances `write_pos`; nothing
  truncates) and replay rescans from block 0 every open. Replay cost grows with lifetime writes.
- `MAX_REPLAY_RECORDS = 100_000` is a **hard fail**, not a throttle: once accumulated writes
  (settingsd prefs alone) cross it, `/state` **refuses to open at boot** (`ReplayLimitExceeded`).
  Compaction is the only fix; this task owns it.

## Progress

**2026-08-15 — plan steps 1+2 landed (engine, host-first; uncommitted).** Journal v2 record
set + committed-only 2PC replay and bounded crash-safe compaction are implemented in
`userspace/statefs` (new `src/journal_v2.rs` 600 LOC, `src/compact.rs` 327 LOC; `protocol`
module split out of `lib.rs` → lib.rs 888 LOC, well under its 1630 baseline). Byte contract
written as normative in `docs/storage/statefs.md` §Journal v2. Design points as fixed by this
ledger: v2 opcodes `0x10–0x14` on the frozen NXSF framing; DELETE stays a committed v1 record
(YELLOW upheld); journal `SYNC` record named `SyncBarrier` apart from the protocol `Sync` op
(YELLOW upheld); `Checkpoint (0x03)` reused as the snapshot boundary. Compaction mirrors the
nxfs checkpoint-flip discipline: snapshot into the inactive half of an A/B region split behind
a new `NXS2` superblock (block 0), atomic single-block flip, zeroed-tail write head — reopen
scans only the live region, so the incremental-replay DoD line holds (op-count assertion:
3 records scanned after N put+compact cycles, independent of N) and `MAX_REPLAY_RECORDS`
becomes unreachable in normal operation. All host DoD bullets for steps 1+2 are covered by
`tests/crash_injection.rs` (SpyDevice harness mirroring nxfs, both-or-neither at every write
cut) + unit tests: 40 lib + 15 crash-injection + 14 envelope green; os-cfg cross-check of
statefs+statefsd clean (statefsd unchanged); structure/dep gates PASS; clippy clean.
Remaining: step 3 fsck-statefs, step 4 statefsd txn wire ops + compaction trigger + OS
selftests/cold-boot gate.

**2026-08-15 — plan step 3 landed (fsck-statefs, host; uncommitted).** Mirrors the nxfs fsck
split exactly: validate/repair CORE in the engine crate (`userspace/statefs/src/fsck.rs`,
485 LOC, no_std-compatible — compiles in the os-lite path, so statefsd could reuse it later)
+ thin host CLI `tools/fsck-statefs/` (`src/main.rs` 126 LOC, `tests/cli.rs` 175 LOC; auto
workspace member via the `tools/*` glob — no root Cargo.toml edit was needed). CLI surface:
`fsck-statefs [--repair] [--dry-run] <journal-image>` on 512-byte-block images
(`statefs::FSCK_BLOCK_SIZE`). Exit codes as per DoD: 0 clean / 1 orphans found-or-repaired /
2 unrecoverable; repair appends one `TXN_ABORT` per orphan (never rewrites committed data),
then re-validates — `repaired` only when the re-validation is clean, else the outcome
degrades to 2. Validation set: v1+v2 layout detection, superblock sanity (`NXS2`
magic/version/CRC/active/geometry), checkpoint↔superblock gen+entries consistency, record
framing/CRC via the engine's own `parse_record`, txn completeness (open-txn mirror of the
replay `TxnTable` membership rules), zeroed-tail discipline (`tail_dirty`, informational),
plus a validator↔replayer record-count cross-check. Semantic line drawn per nxfs discipline:
corruption FOLLOWED by a valid record = mid-journal damage → 2 (byte offset + stable reason:
unknown opcode / record length exceeds caps / crc mismatch / invalid key encoding / invalid
record shape / invalid superblock / …); corruption with nothing valid after it = torn-tail
crash residue replay already discards → reported, not fatal. Inert replay anomalies
(unknown-txn COMMIT/PAYLOAD/ABORT, poisoned txns, out-of-place CHECKPOINT) are counted
(`anomalies`) but never change the exit code — they are not repairable append-only. Engine
crates untouched except: `pub mod fsck` + re-exports in `lib.rs` and three same-line
`pub(crate)` visibility widenings in `compact.rs` (`SUPERBLOCK_MAGIC`, `parse_superblock`,
`region_geometry`); `journal_v2.rs` stays byte-for-byte at its 600-LOC ratchet edge. Proof:
`cargo test -p statefs -p fsck-statefs` all green — statefs 44 lib (40 pre-existing + 4 fsck
unit) + 15 crash-injection + 14 envelope + 16 new `tests/fsck.rs` (outcome matrix incl.
`test_reject_*` negatives), fsck-statefs 9 CLI-contract tests (exit codes, dry-run
no-write, repair→re-fsck-clean, byte-exact fixtures built via the engine API); os-cfg
cross-check statefs+statefsd clean; structure/dep gates PASS; approved fmt; clippy clean
(one pinned-clippy `literal_string_with_formatting_args` FP avoided by hoisting `format!`
out of `assert!` in cli.rs). Remaining: step 4 OS selftests/cold-boot gate (statefsd txn
wire ops + compaction trigger landed in parallel — see its own note).

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
- **replay stays incremental after compaction** (added 2026-08-14): a stored write-head (or
  zeroed tail) makes reopen cost proportional to the live journal, independent of lifetime
  writes — proven by an op-count assertion (records scanned after N put+compact cycles is
  bounded), and the `MAX_REPLAY_RECORDS` hard-fail becomes unreachable in normal operation
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
4. **OS selftest + cold-boot gate** — Reopen-based soft-reboot proof stays; keep-blk cold-boot
   cycle via the `NEXUS_KEEP_BLK=1` harness (landed with TASK-0293 — no longer blocked).
