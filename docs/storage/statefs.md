# StateFS — the `/state` service-state KV store (current state + hardening roadmap)

CONTEXT: Current-state documentation for statefs/statefsd as shipped (TASK-0009 Done,
RFC-0018 Complete, ADR-0023) and the normative home for the v2 extensions
(TASK-0025 envelopes, TASK-0026 journal v2, TASK-0027 record encryption — all Done
2026-08-18).
OWNERS: @runtime
STATUS: v1 + v1b/v2a/v2b shipped and boot-proven (keep-blk double boot); every section below
describes behavior that exists — no contracts-in-waiting remain in this file.

## What statefs is (and is not)

statefs is the **boot-critical service-state KV store** behind the `/state/` namespace:
small values, few writers, replayed fully into RAM at start. It is **not** a user-data filesystem —
user files live in nxfs under `/data` (ADR-0043, RFC-0071). Any PR adding file/path/large-value
semantics here is redirected there.

- Engine (host-first): `userspace/statefs/src/lib.rs` — journal format, replay, IPC protocol module.
- Service: `source/services/statefsd/` (`os_lite.rs` — policy gate, backend selection, audit).
- Contract: `docs/rfcs/RFC-0018-statefs-journal-format-v1.md` (Complete — journal bytes are frozen).

## v1 on-disk format (RFC-0018, shipped)

Append-only journal of records:

```
Magic "NXSF" (4) | OpCode (1) | KeyLen (u16) | ValueLen (u32) | Key | Value | CRC32C (4)
```

- OpCodes: `Put = 0x01`, `Delete = 0x02`, `Checkpoint = 0x03` (parsed, deliberately no-op —
  reserved for v2a compaction).
- Replay: sequential, bounded (`MAX_REPLAY_RECORDS = 100_000`), applies Put/Delete into a
  `BTreeMap`, stops deterministically at the first CRC mismatch or truncated tail.
- Caps: `MAX_KEY_LEN = 255`, `MAX_VALUE_SIZE = 64 KiB` — but the **effective per-value ceiling over
  IPC is ~8 KiB** (frame cap enforced service-side). Plan around 8 KiB.
- Keys are rooted at `/state/` and canonical (`..`/`.` rejected).

## Service surface (shipped)

- Ops: `Put(1) Get(2) Delete(3) List(4: prefix+limit) Sync(5) Reopen(6)` plus the appended
  transaction ops `TxnBegin(7) TxnPut(8) TxnCommit(9) TxnAbort(10)` (TASK-0026, §Journal v2
  → "Service wire ops"); wire v1 plus nonce-correlated v2 framing (RFC-0019) for shared reply
  inboxes.
- Statuses: OK / NOT_FOUND / ACCESS_DENIED / VALUE_TOO_LARGE / KEY_TOO_LONG / INVALID_KEY /
  MALFORMED / IO_ERROR / UNSUPPORTED / INTEGRITY_VIOLATION(9) / ROLLBACK_DETECTED(10) /
  TXN_LIMIT(11).
- Policy: per-op caps `statefs.read`, `statefs.write`, `statefs.keystore` (`/state/keystore/*`),
  `statefs.boot` (`/state/boot/*`) via policyd deny-by-default; denials audited to logd.
- Backend: starts on `MemBlockDevice`, upgrades to virtio-blk while pristine. After ADR-0044 /
  TASK-0293 the block path becomes a `PartitionView` of the GPT `state` partition served by
  `virtioblkd` (journal bytes unchanged).

## Known consumers (keep green through any change)

| consumer | key(s) | note |
|---|---|---|
| keystored | `/state/keystore/device.signing` | boot-critical (chicken-egg for crypto features) |
| updated | `/state/boot/bootctl.v1` | boot-critical |
| settingsd | `/state/settingsd/prefs` | **debt**: hand-rolled wire copy instead of `statefs::client` — migrates in TASK-0025 |
| dsoftbusd | remote `/state` RW gateway | RFC-0030 |

## Durability honesty

The launcher recreates `build/blk.img` on every boot, so plain "persist ok" markers prove
**soft-reboot replay** (Reopen within one VM run), not cold-boot durability. The real
cold-boot proof (TASK-0026) is a double boot against a preserved image:

```bash
just test-os                                          # boot 1 (fresh image): seeds the
                                                      #   sentinel → `SELFTEST: statefs cold-boot seeded`
NEXUS_KEEP_BLK=1 REQUIRE_STATEFS_COLD_BOOT=1 just test-os   # boot 2 (preserved image):
                                                      #   `SELFTEST: statefs cold-boot persist ok` REQUIRED
```

`NEXUS_KEEP_BLK=1` (ADR-0044, wired in TASK-0293) keeps `blk.img`/`data.img` across runs;
`REQUIRE_STATEFS_COLD_BOOT=1` makes the harness treat a `seeded` verdict on the second boot
as the failure it is (persistence silently broken). Without the flag, either verdict passes
the guard — the ok marker only ever appears on a boot that replayed a preserved image.

## Limits of v1 (= the hardening roadmap)

| gap | owner |
|---|---|
| CRC detects corruption, not tampering (no authenticity); no anti-rollback | TASK-0025 |
| ~~no multi-op atomicity (2PC), no compaction, no fsck~~ engine 2PC + bounded compaction (steps 1–2), `fsck-statefs` (step 3) and statefsd txn wire ops + compaction trigger (step 4) landed (§Journal v2) | TASK-0026 |
| ~~values plaintext at rest~~ opt-in record AEAD shipped (§Record encryption v2b; non-boot-critical prefixes, default off) | TASK-0027 |
| no per-subject quotas | TASK-0133 |
| KV snapshots / RO snapshot mounts | TASK-0134 (statefs slice only) |

## §Authenticity envelope v1 (normative; statefsd wiring landed 2026-08-14)

Value-internal envelope (journal bytes untouched):
`{ver, alg, seq (monotonic per key), hmac?, meta{subject, purpose, ts}}` with strict caps;
binary layout + caps in `userspace/statefs/src/envelope.rs` (module header is the byte-level
SSOT). Per-prefix policy off / integrity / authenticated; boot-critical prefixes
(`/state/keystore/*`, `/state/boot/*`) are integrity+seq only (bootstrap chicken-egg —
authenticity there comes from the boot chain, documented not faked). Replay tracks max-seen
`seq` per key; a stale `seq` is a rollback reject (`STATUS_ROLLBACK_DETECTED = 10`); a failed
MAC / downgrade / missing envelope under an authenticated prefix is
`STATUS_INTEGRITY_VIOLATION = 9`.

**Key derivation (v1 contract, `statefs::derive`):** ikm = the deterministic Ed25519
device-key signature over the fixed label `"statefs.envelope.v1"`, then
HKDF-SHA256(salt = info = label) → 32-byte MAC key. statefsd computes the signature locally
from the `/state/keystore/device.signing` record it stores (lazy — before keystored keygen,
authenticated puts fail closed); writers obtain the identical ikm via keystored
`OP_DEVICE_SIGN` — label-scoped: signing the exact derivation label is authorized by the narrow
`crypto.derive.statefs` capability (rule SSOT: `statefs::derive::device_sign_allowed`); any other
payload still requires full `crypto.sign`, so the derivation oracle grants no generic signing
power. The raw key never leaves keystored and is never used as a MAC key directly. v1 boundary:
every `crypto.derive.statefs` (or `crypto.sign`) holder can derive the one envelope key
(per-prefix keys are follow-up work).

**Policy table (v1, `statefsd::hardening`):** authenticated-mandatory = `/state/selftest/secure/`
(fail-closed, no migration window); integrity floor = the boot-critical prefixes above.
**Migration (closed for first-party writers, 2026-08-15):** keystored and updated write
Integrity envelopes for their own keys (`/state/keystore/*` incl. `device.signing`,
`/state/boot/bootctl.v1`) — read-modify-write with seq = last-seen + 1 (first write: seq 1;
shared helper `statefs::writer`, meta subject `keystored`/`updated`). The accept-and-audit
path (`statefsd: envelope migration accept path=…`) now only covers legacy raw bytes still on
the medium (pre-migration journals) and any third-party writer not yet enveloping; reads of
such legacy values pass through, envelope-magic bytes are always verified. Per-write latency budget: 250 ms, overruns audited
(`statefsd: write budget exceeded …`). Full-journal rollback (truncation to before every
tracked seq) still needs an out-of-band anchor (TASK-0289 boot-chain era) — documented, not
claimed.

## §Journal v2 — 2PC (normative; engine landed 2026-08-15, TASK-0026 steps 1–2)

Engine SSOT: `userspace/statefs/src/journal_v2.rs` (records, replay, txn API) and
`userspace/statefs/src/compact.rs` (superblock, regions, compaction). RFC-0018 stays Complete:
every v2 record rides the **unchanged v1 framing**

```
Magic "NXSF" (4) | OpCode (1) | KeyLen (u16 LE) | ValueLen (u32 LE) | Key | Value | CRC32C (4)
```

(CRC32-C over everything before the CRC). v1 records replay byte-identically.

### v2 record set (opcodes + payload shapes; shape violations = Corrupted, replay stops)

| record | op | KeyLen | Value |
|---|---|---|---|
| `PREPARE{txn_id}` | `0x10` | 0 | `txn_id: u64 LE` (8 bytes) |
| `PAYLOAD{txn_id, key, chunk}` | `0x11` | 1..=255 (the target key) | `txn_id: u64 LE ++ chunk` (chunk ≤ 8192 B) |
| `COMMIT{txn_id}` | `0x12` | 0 | `txn_id: u64 LE` (8 bytes) |
| `ABORT{txn_id}` | `0x13` | 0 | `txn_id: u64 LE` (8 bytes) |
| `SYNC{}` | `0x14` | 0 | empty (durability-barrier marker; in code `SyncBarrier`, deliberately named apart from the protocol `Sync` op 5) |
| `CHECKPOINT{gen, entries}` | `0x03` (reused v1 reserve) | 0 | `gen: u32 LE ++ entries: u32 LE` (snapshot boundary, written only by compaction) |

### Replay semantics (committed-only)

- `PAYLOAD` chunks for the same key concatenate in journal order (assembled value ≤ 64 KiB);
  each new key counts toward the per-txn caps.
- `COMMIT` applies all buffered entries of that txn atomically; prepared-without-commit
  (crash/torn tail) is discarded wholesale — never half-applied. Discarded txn ids are burned
  (next id = max seen + 1, nxfs discipline).
- `DELETE` stays its own committed v1 record — non-transactional in v2a (documented YELLOW).
- `CHECKPOINT` resets state (everything before it is superseded); v1 journals never contain it.
- Deterministic anomaly contract (no panic, journal bytes are untrusted): unknown-txn
  `COMMIT`/`PAYLOAD`/`ABORT` → ignored + counted (`JournalEngine::replay_orphans`, feeds the
  later fsck orphan detection); cap violation inside a txn → the txn is *poisoned* (nothing of
  it applies, replay continues); structural violation (bad CRC, unknown opcode, wrong payload
  shape, oversized chunk) → replay stops at that record, everything before is kept.
- Bounds: chunk ≤ 8 KiB (`MAX_TXN_CHUNK`), ≤ 8 open txns, ≤ 32 keys/txn, ≤ 128 KiB/txn,
  ≤ 256 KiB across txns, replay ≤ `MAX_REPLAY_RECORDS = 100_000` per scan.

### Compaction + generations (superblock `NXS2`, A/B regions)

Layouts: **legacy v1** (no superblock; records from block 0 over the whole device; gen 0) and
**v2** from the first compaction on: block 0 holds the superblock
`"NXS2"(4) | ver=1 (1) | active 0=A/1=B (1) | reserved (2) | gen u32 LE | entries u32 LE | crc32c(4)`
(20 bytes), the rest of the device splits into two equal record regions A and B.

A compaction cycle (`compact_now` / threshold-driven `maybe_compact`) writes
`CHECKPOINT{gen+1, n}` + one v1 `Put` per live key (deterministic key order) into the
**inactive** region, syncs, then flips the superblock with a single block write and syncs again —
crash-safe at every cut (the old journal stays intact until the flip; the flip is atomic).
The first compaction of a legacy journal targets region B and requires the v1 journal not to
reach into it (else the cycle is refused/deferred). Threshold: journal bytes ≥
`min_journal_bytes` AND ≥ `live_bytes × ratio` (defaults 8 KiB / 2×); cycles defer while
transactions are open. Work per cycle is O(live entries + snapshot blocks), hard-capped by
`max_entries_per_cycle` — never proportional to lifetime writes (observable via
`CompactionStats{generation, entries, bytes_written, blocks_written}`).

**Incremental replay / zeroed tail:** appends and compaction zero the bytes past the write head
through the end of the following block, so replay stops exactly at the head and stale records
from an earlier generation of a reused region can never be resurrected. Reopen cost is therefore
proportional to the LIVE journal (snapshot + appends since the last compaction), independent of
lifetime writes — the `MAX_REPLAY_RECORDS` hard-fail is unreachable in normal operation.

### Service wire ops (TASK-0026 step 4; appended opcodes, never renumbered)

Wire SSOT: `userspace/statefs/src/protocol/txn.rs`; service glue:
`source/services/statefsd/src/txn.rs` (cfg-free core, host-tested) + `txn_os.rs` (os-lite).
Both wire framings (v1 and nonce-correlated v2) carry the new ops.

| op | request payload | response |
|---|---|---|
| `TXN_BEGIN = 7` | empty | status ++ `txn_id: u64 LE` (0 unless OK) |
| `TXN_PUT = 8` | `txn_id u64 LE ++ key_len u16 ++ chunk_len u32 ++ key ++ chunk` (chunk ≤ 8 KiB) | status |
| `TXN_COMMIT = 9` | `txn_id u64 LE` | status |
| `TXN_ABORT = 10` | `txn_id u64 LE` | status |

Appended status: `STATUS_TXN_LIMIT = 11` — the open-transaction cap (8) is reached at
`TXN_BEGIN`; deliberately distinct from `IO_ERROR` ("retry after another txn commits/aborts"
is not a device failure). Remaining mapping stays on the existing set: unknown txn id →
`NOT_FOUND`; chunk/key/byte caps and assembled value > 64 KiB → `VALUE_TOO_LARGE`; journal
append failure → `IO_ERROR`; malformed frames → `MALFORMED`.

**Policy:** `TXN_BEGIN/COMMIT/ABORT` carry no key and mirror `Sync`/`Reopen`
(`statefs.write` or `statefs.boot`); every `TXN_PUT` key passes the same per-key table as
`Put` (`/state/keystore/*` → `statefs.keystore`, `/state/boot/*` → `statefs.boot`, else
`statefs.write`), denials audited identically.

**Envelope hardening composes at `TXN_PUT` time** (documented choice: verify early — a
forged/stale value never reaches the journal, transactionally or not). Each chunk under an
enrolled prefix runs the same verify-on-put path as `Put`. Consequences: values under
Authenticated prefixes must arrive as a single complete chunk (an envelope cannot be verified
piecewise), and a verified-then-aborted transaction burns the seq (the anti-rollback
high-water mark never rolls back — fail-closed).

**Compaction trigger (statefsd):** after each served request, when `open_txns() == 0`, the
service runs `maybe_compact`. `statefsd: compaction done (gen=<n>, entries=<m>)` is emitted
(UART + logd audit) only after `verify_last_compaction` came back clean — a bounded device
readback that checks the superblock (region/generation/entry count), the checkpoint boundary,
and the fresh snapshot byte-for-byte against what the cycle serialized (held in a persistent
scratch whose capacity is reused across cycles). A failure emits
`statefsd: compaction verify failed` and withholds the marker. The verify deliberately does
NOT re-open the engine: a reopen per cycle materializes a second engine state, and the
os-lite bump allocator never frees — in QEMU that exhausted statefsd's 384 KiB heap at
generation 7 (alloc-fail → service wedged). Byte-equality with the serialized live map plus
deterministic replay proves the same statement with O(one block) memory. Mount marker: `statefsd: journal v2 mounted (2PC)` once, directly
after the journal open() succeeded. The in-VM compaction proof uses the DEFAULT thresholds
(8 KiB / 2×): the selftest's bounded overwrite churn under `/state/app/selftest/compact/`
crosses them deterministically — no test-only threshold exists.

### fsck (offline, landed 2026-08-15, step 3)

Core: `userspace/statefs/src/fsck.rs` (`statefs::fsck`, no_std-compatible, alloc only); thin
host CLI: `tools/fsck-statefs` — `fsck-statefs [--repair] [--dry-run] <journal-image>`
(512-byte-block images). Validates both layouts: superblock sanity, checkpoint↔superblock
consistency (gen + entries), record framing/CRC, txn completeness, zeroed-tail discipline.
Detects orphaned transactions (PREPARE/PAYLOAD without COMMIT/ABORT). Stable exit codes:
**0** clean, **1** orphans found (with `--repair`: retired), **2** unrecoverable. Repair is
append-only — one `TXN_ABORT` per orphan at the write head, committed data is never
rewritten — and counts as repaired only after the repaired journal re-validates clean
(otherwise the outcome degrades to unrecoverable). Unrecoverable = damage replay would stop
on while valid records follow (silent data loss), or invalid/inconsistent v2 metadata; the
report carries the byte offset + a stable reason string. Corruption with nothing valid after
it is torn-tail crash residue: replay already discards it, fsck reports it (`tail_dirty`)
without failing. `--dry-run` prints the repair plan and never writes.

## §Record encryption v2b (normative, TASK-0027)

Opt-in per-prefix AEAD (XChaCha20-Poly1305) of value payloads for **non-boot-critical**
prefixes. Threat model: the block device at rest — the wire carries plaintext (clients never
hold record keys), authorization stays `sender_service_id` + per-key caps. Keys and paths
stay plaintext. No sealed storage exists on this platform (same honesty rule as RFC-0071):
an attacker with the device seed derives the keys.

- **Sealed value (`NXR1` v1, 36-byte overhead):** `magic(4) | ver(1) | class(1) |
  reserved(2) | txn_id u64 LE | chunk_idx u32 LE | ciphertext | tag(16)`. Values are sealed
  at write time and stay sealed in memory and through compaction (snapshots copy
  ciphertext); `get` opens on demand — plaintext out, `EINTEGRITY` (status 9) on any
  tamper/splice.
- **Nonce (24 B, deterministic — no getrandom in the OS graph, RFC-0009):**
  `salt(12) || txn_id LE(8) || chunk_idx LE(4)`. Plain puts consume an id from the txn
  counter; replay re-seeds the counter above every id in PREPARE records AND sealed
  headers, so ids (hence nonces) never repeat across reopen/compaction. Enrolled values in
  transactions are single-chunk (sealing forbids concatenation; the chunk index is the
  txn's entry slot), envelope discipline.
- **AAD** binds the sealed header and the full key path — ciphertext moved to another key,
  txn, or chunk fails to open.
- **Keys:** per class, HKDF-SHA256 over the deterministic Ed25519 device-key signature of
  `statefs.record.v1.<class>` (label = salt = info). Only statefsd derives record keys
  (locally, from the persisted seed) — no keystored oracle, `device_sign_allowed` is
  untouched. Enrolled table (statefsd `enc_svc::ENCRYPTED_PREFIXES`): `/state/app/` →
  class `app`. `/state/keystore/`, `/state/boot/`, `/state/statefsd/` are structurally
  unenrollable (chicken-egg: replay/boot must work before keys exist; the switch itself
  must stay readable).
- **Enablement (default OFF):** the admin meta record `/state/statefsd/enc.v1`
  (`NXEM v1: magic|ver|salt(12)|crc32c`, zero salt rejected). Writing it requires the
  `statefs.admin` capability (per-key cap table row; plain `statefs.write` cannot toggle
  it). The salt is per-store FOREVER — overwriting it would orphan every sealed nonce.
  statefsd enables at mount / after the virtio upgrade / after a device-key or meta write,
  and only after an in-process self-check (real seal → open roundtrip + real tamper
  rejection). Replay-side AEAD verify runs whenever the context is installed (a tampered
  plain put is skipped and counted, a tampered txn chunk poisons its whole transaction);
  the first mount is key-less by construction — sealed values pass through as ciphertext
  and every `get` verifies once keys exist. Disabling never plaintext-ifies anything:
  decryption keys are deterministic, only NEW writes stop being sealed. At-rest tampering
  of the meta record (downgrade of future writes) is inside the at-rest attacker's power
  and outside this task's model — documented, not defended.
- **Markers:** `statefsd: encryption off` (every boot, mem-mount) ·
  `statefsd: encryption on (xchacha20poly1305)` (only after the self-check) ·
  `statefsd: encryption unavailable (keys)` · failure signatures
  `statefsd: enc self-check failed`, `statefsd: enc meta invalid` ·
  `SELFTEST: statefs enc roundtrip ok` (enable + put/get equality before AND after
  Sync+Reopen). The qemu-test.sh guard couples on-marker ↔ roundtrip in both directions;
  FAIL signatures are fatal.
- **fsck:** sealed values are counted (`enc_records`); with
  `--enc-key-hex/--enc-class/--enc-salt-hex` they are AEAD-verified (`enc_failures`,
  exit ≥ 1) — report-only, ciphertext is never rewritten (a keyed replay discards the
  affected txns).
- **Non-goals (unchanged):** key rotation, path/metadata encryption, user-data (`/data`)
  encryption — RFC-0071 P4 / TASK-0320.

## Proof pointers

- Host: `cargo test -p statefs`, `cargo test -p statefsd` (persist + unauthorized contracts).
- QEMU markers: see `scripts/qemu-test.sh` (statefs persist/deny ladder) — extended per task.
