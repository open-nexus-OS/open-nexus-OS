# StateFS — the `/state` service-state KV store (current state + hardening roadmap)

CONTEXT: Current-state documentation for statefs/statefsd as shipped (TASK-0009 Done,
RFC-0018 Complete, ADR-0023) plus the normative home for the v2 extensions that the hardening
tasks keep here (TASK-0025 envelopes, TASK-0026 journal v2, TASK-0027 record encryption).
OWNERS: @runtime
STATUS: v1 shipped and boot-proven; v2 sections below are contracts-in-waiting, clearly marked.

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

- Ops: `Put(1) Get(2) Delete(3) List(4: prefix+limit) Sync(5) Reopen(6)`; wire v1 plus
  nonce-correlated v2 framing (RFC-0019) for shared reply inboxes.
- Statuses: OK / NOT_FOUND / ACCESS_DENIED / VALUE_TOO_LARGE / KEY_TOO_LONG / INVALID_KEY /
  MALFORMED / IO_ERROR / UNSUPPORTED.
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

The launcher recreates `build/blk.img` on every boot, so all current "persist ok" markers prove
**soft-reboot replay** (Reopen within one VM run), not cold-boot durability. Cold-boot proofs
arrive with `NEXUS_KEEP_BLK=1` (ADR-0044, wired in TASK-0293; used by TASK-0026).

## Limits of v1 (= the hardening roadmap)

| gap | owner |
|---|---|
| CRC detects corruption, not tampering (no authenticity); no anti-rollback | TASK-0025 |
| no multi-op atomicity (2PC), no compaction (journal grows forever), no fsck | TASK-0026 |
| values plaintext at rest | TASK-0027 (record AEAD, non-boot-critical prefixes only) |
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

## §Journal v2 — 2PC (normative once TASK-0026 lands)

New opcodes on the v1 framing: `PREPARE{txn_id,…}`, `PAYLOAD{txn_id, chunk}`, `COMMIT{txn_id}`,
`ABORT{txn_id}`, `SYNC{}`. Replay applies committed-only; v1 records keep replaying unchanged;
compaction reuses `Checkpoint (0x03)` as the snapshot boundary and rotates to a fresh journal
(bounded work per cycle). Offline tool: `fsck-statefs` (exit codes 0 ok / 1 repaired /
2 unrecoverable).

## §Record encryption v2b (normative once TASK-0027 lands)

Opt-in per-prefix AEAD (XChaCha20-Poly1305) of value payloads for **non-boot-critical** prefixes;
key = keystored material → HKDF `"statefs.record.v1.<prefix-class>"`; nonce bound to
`(txn_id, chunk_idx)`; AAD binds record header fields. Keys/paths stay plaintext (documented).
Default off; `statefsd: encryption off` when disabled; never claim security without OS entropy.

## Proof pointers

- Host: `cargo test -p statefs`, `cargo test -p statefsd` (persist + unauthorized contracts).
- QEMU markers: see `scripts/qemu-test.sh` (statefs persist/deny ladder) — extended per task.
