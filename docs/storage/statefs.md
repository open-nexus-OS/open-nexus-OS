---
schema: 1
title: StateFS
role: spec
subsystem: storage
sources: [userspace/statefs, source/services/statefsd, tools/fsck-statefs]
contracts: [RFC-0018, RFC-0019, RFC-0072, ADR-0023, ADR-0043]
verified: 2026-11-03
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# StateFS

> **Purpose** — the `/state` service-state KV store: its wire surface, and the normative v2 formats (authenticity envelope, 2PC journal, `NXS2` compaction, record encryption) that no RFC covers.
> **Owns** — `userspace/statefs/`, `source/services/statefsd/`, `tools/fsck-statefs/`
> **Contracts** — [RFC-0018](../rfcs/RFC-0018-statefs-journal-format-v1.md) (v1 record framing, frozen), [RFC-0019](../rfcs/RFC-0019-ipc-request-reply-correlation-v1.md) (nonce-correlated v2 frames), [RFC-0072](../rfcs/RFC-0072-vfs-v2-writable-providers-readdir-stable-errors.md) (quota amendment), [ADR-0023](../adr/0023-statefs-persistence-architecture.md), [ADR-0043](../adr/0043-user-data-in-dedicated-cow-fs-statefs-stays-service-kv.md) (statefs stays a KV)
> **Invariants** — Opcodes and status codes are appended, never renumbered. Every v2 record rides the unchanged v1 framing. No file, path or large-value semantics here — user files go to nxfs.
> **Read next** — [quotas.md](quotas.md), [README.md](README.md), [nxfs.md](nxfs.md)

## What it is

Small values, few writers, replayed fully into RAM at start. Engine: `userspace/statefs/src/lib.rs`
(`JournalEngine`). Service: `source/services/statefsd/` (`os_lite.rs` — policy gate, backend,
audit). Client: `statefs::client::StatefsClient`.

Backend: statefsd opens on a `MemBlockDevice` to reach `ready`, then upgrades to the GPT `state`
partition served by `blkd` (`RemoteBlockDevice`, `route_os.rs`) while still pristine. The upgrade
window is a host-tested state machine (`upgrade_window.rs`); a degrade to RAM-only is announced
once and is fatal in proof profiles.

## Known consumers

| consumer | keys | path |
|---|---|---|
| keystored | `/state/keystore/*` (incl. `device.signing`) | `statefs::writer` Integrity envelopes; boot-critical |
| bootctld | `/state/boot/bootctl.v1` | sole owner (ADR-0055); `updated` reaches it only through bootctld |
| settingsd | `/state/settingsd/prefs` | `statefs::client::StatefsClient` |
| logd | `/state/logd/evidence/*` | evidence spill ring |
| execd | `/state/crash/*` | crash artifacts, `statefs.crash` |
| metricsd | `/state/observability/metricsd/*` | WAL + rollups |
| init | `/state/init/restarts/*` | restart counters |
| apps (`restorable = true`) | `/state/app/@self/*` | SDK route `svc.state` from app-host; `@self` is rewritten to the sender's kernel identity (`app_key.rs`) |
| dsoftbusd | remote `/state` gateway | RFC-0030 |

## Limits

- `MAX_KEY_LEN = 255`, keys rooted at `/state/`, canonical (`.`/`..` rejected).
- `MAX_VALUE_SIZE = 64 KiB` in the engine, but one IPC frame carries at most
  `IPC_PAYLOAD_MAX` (8 KiB): a plain put is ≤ `MAX_INLINE_VALUE_BYTES` (frame − 64). Larger values
  go through a transaction in ≤ 8 KiB chunks.
- Replay is bounded: `MAX_REPLAY_RECORDS = 100_000` per scan.

## Wire surface

Wire v1 plus nonce-correlated v2 framing (RFC-0019) for shared reply inboxes; every op exists in
both. SSOT: `userspace/statefs/src/protocol.rs`, `protocol/txn.rs`.

| op | code | notes |
|---|---|---|
| Put / Get / Delete | 1 / 2 / 3 | |
| List | 4 | prefix + limit (≤ 256) |
| Sync / Reopen | 5 / 6 | Reopen = soft-reboot replay |
| TxnBegin / TxnPut / TxnCommit / TxnAbort | 7 / 8 / 9 / 10 | §Journal v2 |
| FsckCheck / FsckRepair | 11 / 12 | online fsck; both `STATUS_BUSY` while a txn is open |

Statuses: `OK 0`, `NOT_FOUND 1`, `ACCESS_DENIED 2`, `VALUE_TOO_LARGE 3`, `KEY_TOO_LONG 4`,
`INVALID_KEY 5`, `MALFORMED 6`, `IO_ERROR 7`, `UNSUPPORTED 8`, `INTEGRITY_VIOLATION 9`,
`ROLLBACK_DETECTED 10`, `11` (= `TXN_LIMIT` on `TxnBegin`, `BUSY` on the fsck ops),
`QUOTA_EXCEEDED 12` ([quotas.md](quotas.md)).

### Policy (per-key capability table)

Checked against the kernel `sender_service_id` via policyd, deny-by-default, denials audited:

| key prefix | mutating ops need |
|---|---|
| `/state/keystore/` | `statefs.keystore` |
| `/state/boot/` | `statefs.boot` (reads too) |
| `/state/crash/` | `statefs.crash` |
| `/state/statefsd/` (admin keys) | `statefs.admin` |
| `/state/app/<own identity>/` | the `svc.state` route itself |
| anything else | `statefs.write` (reads: `statefs.read`) |

`TxnBegin/Commit/Abort` carry no key and mirror `Sync`/`Reopen`; every `TxnPut` key passes the
same table (`txn.rs::required_txn_put_cap`). `FsckCheck` needs `statefs.read`, `FsckRepair`
`statefs.admin`.

## v1 record framing (RFC-0018)

```
Magic "NXSF" (4) | OpCode (1) | KeyLen (u16 LE) | ValueLen (u32 LE) | Key | Value | CRC32C (4)
```

v1 ops: `Put 0x01`, `Delete 0x02`, `Checkpoint 0x03`. Replay applies Put/Delete into a `BTreeMap`
and stops at the first CRC mismatch or truncated tail.

## §Authenticity envelope (normative)

Value-internal (journal bytes untouched): `{ver, alg, seq, hmac?, meta{subject, purpose, ts}}`.
Byte layout and caps: module header of `userspace/statefs/src/envelope.rs` (byte-level SSOT).

- Per-prefix class (`statefsd::hardening`): **Authenticated** (MAC mandatory) =
  `/state/selftest/secure/`; **Integrity** floor (seq, `alg = none`) = `/state/keystore/`,
  `/state/boot/`. Boot-critical prefixes carry no MAC: their authenticity comes from the boot
  chain (bootstrap chicken-and-egg).
- Replay tracks the max-seen `seq` per key. A stale `seq` → `ROLLBACK_DETECTED (10)`. A failed MAC,
  downgrade or missing envelope under an Authenticated prefix → `INTEGRITY_VIOLATION (9)`.
- Writers use `statefs::writer`: read-modify-write, `seq = last_seen + 1`, first write `seq = 1`.
  Legacy raw bytes classify as `Legacy` and are accepted with an audit line
  (`statefsd: envelope migration accept path=…`).
- **MAC key:** ikm = the deterministic Ed25519 device-key signature over the label
  `"statefs.envelope.v1"`; HKDF-SHA256(salt = info = label) → 32 bytes. statefsd computes it from
  the stored `/state/keystore/device.signing` record (before keygen, authenticated puts fail
  closed). Writers get the same ikm from keystored `OP_DEVICE_SIGN`; signing exactly that label
  needs only `crypto.derive.statefs` (`statefs::derive::device_sign_allowed`), any other payload
  needs `crypto.sign`. Every holder of that capability derives the one envelope key.
- Write budget: 250 ms per put (`WRITE_BUDGET_NS`), overruns audited.
- Not covered: rollback of the whole journal to before every tracked seq (needs an out-of-band
  anchor).

## §Journal v2 — 2PC (normative)

Engine SSOT: `userspace/statefs/src/journal_v2.rs` (records, replay, txn API),
`userspace/statefs/src/compact.rs` (superblock, regions). v1 records replay byte-identically.

### Records (shape violation = corrupted, replay stops)

| record | op | KeyLen | Value |
|---|---|---|---|
| `PREPARE{txn_id}` | `0x10` | 0 | `txn_id u64 LE` |
| `PAYLOAD{txn_id, key, chunk}` | `0x11` | 1..=255 (target key) | `txn_id u64 LE ++ chunk` (≤ 8192 B) |
| `COMMIT{txn_id}` | `0x12` | 0 | `txn_id u64 LE` |
| `ABORT{txn_id}` | `0x13` | 0 | `txn_id u64 LE` |
| `SYNC{}` | `0x14` | 0 | empty — durability barrier (`SyncBarrier` in code, distinct from wire op 5) |
| `CHECKPOINT{gen, entries}` | `0x03` | 0 | `gen u32 LE ++ entries u32 LE` — written only by compaction |

### Replay (committed-only)

- `PAYLOAD` chunks for one key concatenate in journal order (≤ 64 KiB assembled).
- `COMMIT` applies the txn atomically. Prepared-without-commit is discarded wholesale. Discarded
  ids are burned: next id = max seen + 1.
- `DELETE` stays a non-transactional v1 record.
- `CHECKPOINT` resets state; everything before it is superseded.
- Anomalies never panic (journal bytes are untrusted):
  - unknown-txn `COMMIT`/`PAYLOAD`/`ABORT` → ignored, counted (`replay_orphans`);
  - cap violation inside a txn → txn poisoned, replay continues;
  - bad CRC, unknown opcode, wrong shape, oversized chunk → replay stops there, earlier records kept.
- Bounds: `MAX_TXN_CHUNK` 8 KiB, `MAX_OPEN_TXNS` 8, `MAX_TXN_KEYS` 32, `MAX_TXN_BYTES` 128 KiB,
  `MAX_TXN_TOTAL_BYTES` 256 KiB.

### Wire ops

| op | request | response |
|---|---|---|
| `TXN_BEGIN 7` | empty | status ++ `txn_id u64 LE` (0 unless OK) |
| `TXN_PUT 8` | `txn_id u64 ++ key_len u16 ++ chunk_len u32 ++ key ++ chunk` | status |
| `TXN_COMMIT 9` | `txn_id u64` | status |
| `TXN_ABORT 10` | `txn_id u64` | status |

Errors: open-txn cap at `TXN_BEGIN` → `TXN_LIMIT (11)` (retry later, not a device failure);
unknown id → `NOT_FOUND`; chunk/key/byte caps or > 64 KiB assembled → `VALUE_TOO_LARGE`; append
failure → `IO_ERROR`; bad frame → `MALFORMED`. Service glue: `source/services/statefsd/src/txn.rs`
(host-tested) + `txn_os.rs`.

Envelopes are verified at `TXN_PUT` time, so a forged or stale value never reaches the journal.
Consequences: an Authenticated value arrives as one complete chunk; a verified-then-aborted txn
burns its seq.

### Compaction (`NXS2` superblock, A/B regions)

- **Legacy v1 layout:** no superblock, records from block 0, generation 0.
- **v2 layout** (from the first compaction): block 0 =
  `"NXS2"(4) | ver=1 (1) | active 0=A/1=B (1) | reserved (2) | gen u32 LE | entries u32 LE | crc32c (4)`
  (20 bytes); the rest splits into two equal record regions A and B.
- A cycle writes `CHECKPOINT{gen+1, n}` + one v1 `Put` per live key (key order) into the
  **inactive** region, syncs, flips the superblock with one block write, syncs. The old journal
  stays intact until the flip. The first compaction of a legacy journal targets B and is deferred
  if the v1 journal reaches into B.
- Trigger: journal bytes ≥ `min_journal_bytes` AND ≥ `live_bytes × ratio` (defaults 8 KiB / 2×),
  never while a txn is open. Work per cycle is O(live entries), capped by `max_entries_per_cycle`.
- Zeroed tail: appends and compaction zero past the write head through the end of the next
  block, so replay stops at the head and stale records of a reused region never come back. Reopen
  cost is proportional to the live journal, not lifetime writes.
- statefsd runs `maybe_compact` after each request when no txn is open. It emits
  `statefsd: compaction done (gen=<n>, entries=<m>)` only after `verify_last_compaction` — a
  bounded readback of superblock, checkpoint and snapshot bytes against what the cycle serialized
  (no engine reopen: that would materialize a second state on a heap that does not free).
  Failure: `statefsd: compaction verify failed`, no done marker.

### fsck

Core `userspace/statefs/src/fsck.rs` (no_std + alloc, streamed through `fsck_window.rs`); CLI
`tools/fsck-statefs` — `fsck-statefs [--repair] [--dry-run] <journal-image>` (512 B blocks);
online via ops 11/12.

- Checks: superblock, checkpoint ↔ superblock (gen + entries), framing/CRC, txn completeness,
  zeroed tail; finds orphaned txns (PREPARE/PAYLOAD without COMMIT/ABORT).
- Exit codes: **0** clean, **1** orphans (with `--repair`: retired), **2** unrecoverable.
- Repair is append-only: one `ABORT` per orphan at the head, committed data never rewritten; it
  counts only if the repaired journal re-validates clean.
- Unrecoverable = damage replay would stop on while valid records follow, or bad v2 metadata
  (report carries byte offset + stable reason). Corruption with nothing valid after it is a torn
  tail: reported (`tail_dirty`), not a failure.

## §Record encryption (normative)

Opt-in per-prefix AEAD (XChaCha20-Poly1305) of values under non-boot-critical prefixes. Threat
model: the block device at rest. The wire carries plaintext; keys and paths stay plaintext. No
sealed key storage exists: an attacker with the device seed derives the keys.

- **Sealed value `NXR1` v1** (36-byte overhead):
  `magic(4) | ver(1) | class(1) | reserved(2) | txn_id u64 LE | chunk_idx u32 LE | ciphertext | tag(16)`.
  Sealed at write, kept sealed in memory and through compaction; `get` opens on demand,
  `INTEGRITY_VIOLATION (9)` on tamper or splice.
- **Nonce (24 B, deterministic):** `salt(12) || txn_id LE(8) || chunk_idx LE(4)`. Plain puts take
  an id from the txn counter; replay re-seeds the counter above every id seen in PREPARE records
  and sealed headers, so nonces never repeat. Enrolled values in a txn are single-chunk.
- **AAD:** the sealed header + the full key path.
- **Keys:** per class, HKDF-SHA256 over the device-key signature of
  `statefs.record.v1.<class>` (label = salt = info). Only statefsd derives them, locally.
- **Enrolled:** `enc_svc::ENCRYPTED_PREFIXES` = `/state/app/` → class `app`. `/state/keystore/`,
  `/state/boot/`, `/state/statefsd/` cannot be enrolled.
- **Enablement (default off):** admin record `/state/statefsd/enc.v1`
  (`NXEM v1: magic | ver | salt(12) | crc32c`, zero salt rejected), writable only with
  `statefs.admin`. The salt is fixed for the store's life. statefsd enables after a seal → open →
  tamper-reject self-check. Disabling only stops sealing new writes. Tampering with the meta record
  at rest is out of scope.
- **Markers:** `statefsd: encryption off` · `statefsd: encryption on (xchacha20poly1305)` ·
  `statefsd: encryption unavailable (keys)` · `statefsd: enc self-check failed` ·
  `statefsd: enc meta invalid` · `SELFTEST: statefs enc roundtrip ok`.
- **fsck:** counts sealed values (`enc_records`); with `--enc-key-hex/--enc-class/--enc-salt-hex`
  AEAD-verifies them (`enc_failures`, exit ≥ 1), report-only.
- Not here: key rotation, path encryption, `/data` encryption (RFC-0071 Phase 4).

## Durability proof

The launcher rewrites `build/nexus.img` on every boot unless `NEXUS_KEEP_BLK=1`, so plain persist
markers prove soft-reboot replay only. Cold boot is a double boot:

```bash
just test-os                                                 # boot 1: SELFTEST: statefs cold-boot seeded
NEXUS_KEEP_BLK=1 REQUIRE_STATEFS_COLD_BOOT=1 just test-os    # boot 2: SELFTEST: statefs cold-boot persist ok REQUIRED
```

With `REQUIRE_STATEFS_COLD_BOOT=1` a `seeded` verdict on boot 2 fails the run.

## Proof

- Host: `cargo test -p statefs`, `cargo test -p statefsd`.
- QEMU: `statefsd: journal v2 mounted (2PC)`, `statefsd: compaction done (gen=`, the statefs
  markers in `source/apps/selftest-client/proof-manifest/markers/bringup.toml`; guards in
  `scripts/qemu-test.sh`.
