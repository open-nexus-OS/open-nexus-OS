---
title: TASK-0026 StateFS v2a: 2PC crash-atomicity + bounded compaction + fsck tool (rebased 2026-07-15 onto shipped statefs v1)
status: Done
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

**2026-08-15 — plan steps 1+2 landed (engine, host-first; committed d5d3252b).** Journal v2 record
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

**2026-08-15 — plan step 3 landed (fsck-statefs, host; committed d5d3252b).** Mirrors the nxfs fsck
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

## Progress — 2026-08-15: plan step 4 landed (statefsd txn wire ops + compaction trigger + OS selftests; committed d5d3252b)

**Wire ops (appended, never renumbered; SSOT `userspace/statefs/src/protocol/txn.rs`, a
submodule — `protocol.rs` itself gained only the `pub mod txn;` declaration, engine modules
untouched):** `TXN_BEGIN = 7` (empty payload → status ++ `txn_id: u64 LE`, 0 unless OK),
`TXN_PUT = 8` (`txn_id u64 ++ key_len u16 ++ chunk_len u32 ++ key ++ chunk`, chunk ≤ 8 KiB),
`TXN_COMMIT = 9` / `TXN_ABORT = 10` (`txn_id u64`). Both framings (v1 + nonce v2) carried.
One appended status: `STATUS_TXN_LIMIT = 11` (open-txn cap at BEGIN — the existing set could
not distinguish "retry later" from a device failure); everything else maps onto the existing
set (unknown id → NOT_FOUND, caps → VALUE_TOO_LARGE, append failure → IO_ERROR). Documented
in `docs/storage/statefs.md` §Journal v2 → "Service wire ops".

**statefsd:** cfg-free core `src/txn.rs` (status mapping, per-key cap table for TXN_PUT,
compaction tick — host-tested like `hardening.rs`; `storage` added to the std feature for the
`BlockDevice` generic) + os-lite glue `src/txn_os.rs` (policy: BEGIN/COMMIT/ABORT =
`statefs.write`-or-`statefs.boot` mirroring Sync; every TXN_PUT key runs the same
keystore/boot/write table as Put with the canonical-subject mapping). Envelope hardening
composes at **TXN_PUT time** (documented decision: fail early; Authenticated values must be a
single complete chunk; a verified-then-aborted txn burns the seq — fail-closed).
`os_lite.rs` delta stayed small (613 LOC vs 622 baseline): txn dispatch, mount marker, txn ops
in the pristine list, one `compaction_tick` call after each served request.

**Compaction trigger (no-fake-green):** the ledger's earlier note claimed the engine
re-replays the rotated journal in its flip discipline — **verified FALSE in `compact.rs`**
(`compact_now` snapshots + flips, no re-replay). Not an engine bug (the flip is crash-safe),
but the marker honesty is now provided service-side: `txn::compaction_tick` runs
`maybe_compact` only when `open_txns() == 0`, and on `Some(stats)` re-opens the engine (full
replay of the rotated journal) and checks `generation()` + `len()` against the cycle stats;
only then `statefsd: compaction done (gen=<n>, entries=<m>)` is emitted (UART + logd audit).
Mismatch → `statefsd: compaction verify failed` (new failure signature), marker withheld.
Mount marker `statefsd: journal v2 mounted (2PC)` emits once, directly after a successful
`open()`. **In-VM threshold approach: DEFAULT config (min 8 KiB, ratio 2×) — no test knob.**
The selftest churns 6 keys × 16 rounds × 256 B values (~29 KiB journal per batch, ≤ 4 bounded
batches) under `/state/app/selftest/compact/`; overwrites keep live bytes flat so the ratio
gate is crossed deterministically; the between-requests tick keeps the journal from ever
outgrowing even the 32 KiB mem-fallback region.

**Selftests (declared in `proof-manifest/markers/bringup.toml`, emitted from
`phases/bringup.rs` via probes in `services/statefs_v2.rs`):**
`SELFTEST: statefs v2 crash-atomic ok|FAIL` (delete-clean → BEGIN+2×PUT, invisible-before-
commit checked, ABORT → both NOT_FOUND; then BEGIN+2×PUT+COMMIT → visible, Sync+Reopen →
still visible), `SELFTEST: statefs v2 compact ok|FAIL` (churn until the logd-audited
`statefsd: compaction done (gen=` line exists — no fake trigger — then Reopen + verify every
churn key carries its last-written value), `SELFTEST: statefs cold-boot persist ok|FAIL` +
info line `SELFTEST: statefs cold-boot seeded` (sentinel `/state/app/selftest/
coldboot.sentinel`: absent → put+sync+`seeded`; present-correct → ok, i.e. only ever on a
NEXUS_KEEP_BLK=1 second boot; present-wrong → FAIL). Service markers
`statefsd: journal v2 mounted (2PC)`, `statefsd: compaction done (gen=`,
`statefsd: compaction verify failed` also declared in the manifest.

**Host proof:** `cargo test -p statefs -p statefsd` fully green — statefs suites untouched-
green (44 lib + 15 crash_injection + 14 envelope + 16 fsck) and statefsd 2 lib + 13 hardening
+ 8 persist + 8 unauthorized + **11 new `tests/txn_contract.rs`** (commit-visible/survives-
reopen, abort discards incl. after replay, unknown-id NOT_FOUND, begin-over-cap TXN_LIMIT,
oversize chunk VALUE_TOO_LARGE engine+wire, envelope deny inside txn never reaches the
buffer, cap table, compaction tick threshold/stats/reopen-verify + incremental-replay
op-count, defers-while-txn-open, wire roundtrips v1+v2, malformed frames). Gates: os-cfg
riscv64 os-lite check of statefsd + selftest-client clean under `-D warnings`; host warnings
gate (all-targets) clean; `just structure-gate` PASS (os_lite.rs 613 ≤ 622 baseline);
`just dep-gate` PASS; selftest arch-gate 6/6; approved fmt; repo-gate clippy clean.
NOT run here (main session): marker registration in `scripts/qemu-test.sh` /
`tools/nx/chains/markers.txt` + the QEMU ladder incl. the keep-blk double boot.

**QEMU proof must check:** `statefsd: journal v2 mounted (2PC)` (once, before ready),
`SELFTEST: statefs v2 crash-atomic ok`, `statefsd: compaction done (gen=` then
`SELFTEST: statefs v2 compact ok`; boot 1 (fresh blk) → `SELFTEST: statefs cold-boot seeded`,
boot 2 (NEXUS_KEEP_BLK=1) → `SELFTEST: statefs cold-boot persist ok`. Failure signatures worth
grepping: `statefsd: compaction verify failed`, `SELFTEST: statefs v2 crash-atomic FAIL`,
`SELFTEST: statefs v2 compact FAIL`, `SELFTEST: statefs cold-boot persist FAIL`, and
`statefsd: err io` during the churn (mem-fallback region overflow = virtio upgrade never
happened). Remaining in-task scope: step 3 fsck tool wiring (parallel) + harness/marker
registration + keep-blk ladder run.

## Progress — 2026-08-18: QEMU round 1 found a real bug — reopen-verify OOM'd statefsd

First ladder run (markers + TASK-0026 fake-green guard registered in `scripts/qemu-test.sh`,
0025-guard pattern; cold-boot verdict gated by the guard — `seeded` on a fresh image,
`persist ok` required only under `REQUIRE_STATEFS_COLD_BOOT=1`): **red, honestly.**
`statefsd: compaction done (gen=7 …)` then `alloc-fail svc=statefsd size=0x12d` with the
384 KiB bump heap exhausted → recv timeout → `SELFTEST: statefs v2 compact FAIL` → every
later persist consumer failed (updated/OTA cascade). Root cause: the compaction tick's
honesty device was `engine.reopen()` — a FULL replay materializing a second engine state
every cycle, and the os-lite bump allocator never frees. Ambient bringup writes cross the
default 8 KiB threshold repeatedly, so 7 cycles/boot × whole-store duplication killed the
heap. **Fix (production-grade, not a heap bump):** `compact_now` now serializes into a
persistent engine scratch (capacity reused across cycles), and the new
`statefs::JournalEngine::verify_last_compaction` re-reads superblock + snapshot from the
device and byte-compares against that scratch (checkpoint parse + zeroed-tail check, O(one
block) memory). `txn::compaction_tick` uses it instead of reopen; marker semantics unchanged
(`compaction done` only after clean readback, else `compaction verify failed`). New
adversarial proof `test_reject_compaction_verify_detects_tampered_readback` (TamperDevice:
corrupted snapshot readback, corrupted superblock readback, wrong stats → all withhold the
marker; clean readback verifies). Host: statefs 44+16+14+16, statefsd 2+13+8+8+11, fsck 9 —
all green; clippy clean. Docs updated (statefs.md §Compaction trigger, manifest proves-text).

**Round 2 findings (same day):** (a) the heap fix alone was not enough — the PUT hot path
itself allocated per request (fresh `encode_record` Vec + fresh 512 B RMW block buffer +
value clone per overwrite), so the churn probe still starved the heap without any reopen.
Fixed engine-side: new `statefs::record::encode_record_into` (append-in-place; journal_v2.rs
shrank 600→587 by moving the encoder out), persistent record/block scratches in the engine,
and value-capacity reuse on overwrites — steady-state overwrite churn now allocates nothing
per put (QEMU heap watermark 50% after 20 compaction generations). (b) the compact probe's
logd cross-check sat on the rotted logd-query lane (same helper as the long-pre-existing
`SELFTEST: metrics retention` FAIL) and failed while compaction demonstrably ran (gen=19 on
UART). Rewired: the probe proves churn + post-compaction replay integrity over the wire;
cycle-ran proof is owned by the harness (`statefsd: compaction done (gen=` = required marker
+ fake-green guard, emitted only after the service's bounded readback verify). A probe must
not gate on a side channel that is broken independently of the behavior under test.
**QEMU boot 1 (fresh image): GREEN** — exit 0, `journal v2 mounted (2PC)`, `crash-atomic ok`,
`compact ok`, `cold-boot seeded`, 20× `compaction done`, no alloc-fail, no verify-failed
(headless--2026-08-18T12-27-19).

## Closure — 2026-08-18: Done

All five OS/QEMU DoD markers proven by the final double-boot sequence (both runs exit 0):

- boot 1, fresh image (headless--2026-08-18T12-32-13): `statefsd: journal v2 mounted (2PC)`,
  `SELFTEST: statefs v2 crash-atomic ok`, `statefsd: compaction done (gen=…)`,
  `SELFTEST: statefs v2 compact ok`, `SELFTEST: statefs cold-boot seeded`
- boot 2, preserved image `NEXUS_KEEP_BLK=1 REQUIRE_STATEFS_COLD_BOOT=1`
  (headless--2026-08-18T12-33-47): `SELFTEST: statefs cold-boot persist ok` — real cold-boot
  durability, plus all 0025 hardening markers still green on the preserved journal
  (via the `next_base_seq` probe fix).

Harness: 5 markers in `scripts/qemu-test.sh` `expected_sequence` (full + headless mount
marker), TASK-0026 fake-green guard (ok-markers required once the mount marker appears,
FAIL variants + `statefsd: compaction verify failed` fatal, cold-boot verdict required,
`REQUIRE_STATEFS_COLD_BOOT=1` strictness for the second boot). `tools/nx/chains/markers.txt`
deliberately untouched (chain markers only — same call as 0025). Host proof totals:
statefs 44 lib + 16 crash_injection + 14 envelope + 16 fsck; statefsd 2 + 13 + 8 + 8 + 11
txn_contract; fsck-statefs 9 CLI. Docs: statefs.md (§Journal v2 wire ops, §Compaction
trigger readback-verify, §Durability honesty double-boot recipe), CHANGELOG, this ledger.
Follow-ups unchanged: TASK-0027 (record encryption, now unblocked), TASK-0134 (snapshots).
Known debt recorded, not in scope: the logd query lane is rotted (pre-existing metrics
FAILs) — the compact probe no longer depends on it.

**Round 3 finding (keep-blk boot 2):** `SELFTEST: statefs cold-boot persist ok` appeared
(the double-boot proof itself works), but the run failed on the 0025 guard —
`statefs auth put FAIL` with `envelope deny status=10`: the hardening probes' base seq was
purely boot-time (`nsec`), which restarts near zero, while the PRESERVED journal (and
statefsd's replay-fed anti-rollback tracker) carries boot 1's higher seq. The in-code
comment even said "blk.img is wiped per test boot" — no longer true under keep-blk. Fixed
probe-side with `statefs_hardening::next_base_seq` (GET + envelope decode, base =
`max(nsec, stored_seq + 1)` — the same discipline as `statefs::writer`'s SeqCache from the
0025 bootctl fix). Re-proving with a fresh boot-1 + keep-blk boot-2 sequence.
