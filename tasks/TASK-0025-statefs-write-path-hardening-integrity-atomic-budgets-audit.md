---
title: TASK-0025 StateFS write-path hardening: authenticity envelopes + anti-rollback + budgets (rebased 2026-07-15 onto shipped statefs v1)
status: Done
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
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
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

**Verified still open (2026-08-14, storage reconciliation):** repo reality unchanged — no
envelope/HMAC/seq code in `userspace/statefs`, and settingsd still speaks the hand-rolled wire
copy. Note the nxfs ladder (TASK-0314–0320) does not cover this: statefs stays a separate,
boot-critical store per ADR-0043, so this lane (0025 → 0026 → 0027) still has to run.

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
- `docs/storage/statefs.md`, `docs/testing/README.md`, `scripts/qemu-test.sh`

## Plan (small PRs)

1. Envelope v1 in `userspace/statefs` (host-first): `{ver, alg, seq, hmac?, meta{subject,purpose,ts}}`,
   CBOR-or-fixed-struct encoding, strict caps; wrap/verify + replay-time max-seq tracking.
2. statefsd: per-prefix envelope policy (off / integrity / authenticated), budgets + warn path.
3. settingsd client-crate migration (independent, lands first — pure debt payoff).
4. keystored/updated adoption + selftest markers.

## Progress

### 2026-08-14 — plan step 2 landed (statefsd wiring + selftests + host proofs); QEMU proof pending

Step 2 is code-complete and host-proven; status stays **In Progress** until the QEMU markers
are registered (`scripts/qemu-test.sh` + `tools/nx/chains/markers.txt`, main session) and boot-proven.

- **Key derivation contract (v1, decided here):** `EnvelopeKey = HKDF-SHA256(ikm, salt=info=
  "statefs.envelope.v1")` with **ikm = the deterministic Ed25519 device-key signature over the
  label**. Rationale: keystored refuses raw-key export by design, and statefsd→keystored IPC
  from inside the serve loop is a deadlock-shaped dependency (keystored is a statefsd client).
  So statefsd signs locally with the `/state/keystore/device.signing` record it already stores
  (lazy; before keygen, Authenticated ops fail closed = chicken-egg rule), while writers get
  the identical ikm from keystored `OP_DEVICE_SIGN` (existing wire op; authorization is
  label-scoped, see the 2026-08-15 note below). Shared HKDF lives in
  `statefs::derive` (new `hkdf` dep, RustCrypto no_std, dep-gate green). Documented v1
  boundary: one envelope key per device; any derivation-cap holder can derive it.
- **statefsd** (`src/hardening.rs` new, cfg-free + host-tested; `src/emit_os.rs` split out of
  the grandfathered `os_lite.rs`, now 592 LOC ≤ 622): const policy table
  (`AUTHENTICATED_PREFIXES = ["/state/selftest/secure/"]` + Integrity floor via
  `default_for_key`), verify-on-put ordered decode → MAC → `check_put` (a forged value never
  advances the seq high-water mark), verify-on-get (`check_read` + MAC), replay-time
  `observe_enrolled` walk (bounded, re-run on the mem→virtio upgrade), migration
  accept-and-audit for raw bytes under the Integrity floor (keystored/updated keep booting),
  `WriteBudget` (250 ms) wired with `nsec()` around `engine.put` + audit warn line, envelope
  denials audited via logd (`statefsd: envelope deny path=… status=…`, no payload/keys).
  Marker `statefsd: write hardening on (auth-envelope)` emitted once after tracker feed +
  hardening init, directly after `statefsd: ready`.
- **selftest-client:** `services/statefs_hardening.rs` derives the key via keystored keygen
  (idempotent) + `OP_DEVICE_SIGN`, then proves: authenticated put accepted + get returns the
  sealed bytes with client-side MAC verify + payload equality (`SELFTEST: statefs auth put ok`);
  payload bit-flip → status 9 (`SELFTEST: statefs tamper deny ok`); stale seq with valid MAC →
  status 10 (`SELFTEST: statefs rollback deny ok`). FAIL variants emitted on every failure
  path. Markers declared in `proof-manifest/markers/bringup.toml`.
- **Host proofs:** `cargo test -p statefs -p statefsd` green — 13 new tests in
  `statefsd/tests/hardening_contract.rs` incl. `test_reject_forged_value_integrity_violation`,
  `test_reject_stale_seq_rollback_detected`, `test_reject_oversize_envelope_meta`,
  `test_reject_authenticated_without_key_fail_closed`,
  `test_reject_downgrade_integrity_envelope_on_authenticated_prefix`,
  `test_reject_raw_bytes_on_authenticated_prefix_no_migration_window`, budget-warn hook, and
  `test_derivation_matches_keystored_sign_oracle` (statefsd-local derivation ≡ writer-side).
- **Collateral integration:** the new `StatefsError::{IntegrityViolation,RollbackDetected}`
  variants broke exhaustive matches in keystored/updated OS builds — fixed (keystored maps
  both to `STATUS_DENY`; updated now uses the new `StatefsError::label()` SSOT). All four
  crates compile clean under the os cfg (riscv64, `--features os-lite`, zero warnings);
  `just structure-gate`, `just dep-gate`, `just arch-check`, clippy all PASS.
- **Remaining for step 2 DoD (main session):** register the four markers in
  `scripts/qemu-test.sh` / `tools/nx/chains/markers.txt`; run the QEMU ladder for
  `statefsd: write hardening on (auth-envelope)` + the three SELFTEST markers.

### 2026-08-15 — QEMU run 1: hardening markers green; over-broad `crypto.sign` grant fixed (label-scoped derivation cap)

QEMU (`build/logs/headless--2026-08-15T12-01-25`): all four new markers green, but the broad
`crypto.sign` grant to selftest-client regressed `SELFTEST: keystored sign denied` (raw
`OP_SIGN` with an arbitrary payload was no longer denied) — the deny test is right: the grant
handed the test subject generic device-signing power just to derive one MAC key.

- **Fix (label-scoped authorization, SSOT in `statefs::derive::device_sign_allowed`):**
  keystored `handle_device_sign` now authorizes the EXACT derivation label under the new
  narrow cap `crypto.derive.statefs` (or `crypto.sign`); any other payload keeps requiring
  full `crypto.sign`. `OP_SIGN` (op 5) is untouched and still requires `crypto.sign`.
  `policies/base.toml`: selftest-client's `crypto.sign` replaced by `crypto.derive.statefs`.
  statefsd (signs locally) needs no cap. Identity stays the kernel IPC sender; the policy
  check is injected (`FnMut(&str) -> bool`), the rule lives next to the label so oracle and
  contract cannot drift. Host `test_reject_non_label_sign_with_only_derive_cap` (+ label/no-cap
  and near-miss-label negatives) in `statefs::derive` tests.
- **Why `SELFTEST: policy deny ok` (policyd OP_CHECK_CAP selftest-client/crypto.sign) still
  passed while keystored allowed:** it was a false-pass by probe construction, not a second
  policy source. Both paths consult the same generated table with the same
  `normalize_subject_id` canonicalization, and the table demonstrably contained the grant
  (the derivation markers passed through the delegated check). The deny probe computes
  `policyd_check_cap(..).unwrap_or(false) == false` with a 100 ms recv timeout — any transport
  error, late reply, or non-ALLOW status (incl. MALFORMED) counts as "denied". So the marker
  can pass without policyd ever evaluating a deny; it proves "no ALLOW observed within
  100 ms", not "DENY decided". Worth hardening in a selftest-focused task (out of 0025 scope).
- Verification: `cargo test -p statefs -p statefsd -p keystored` = 115 passed / 0 failed
  (incl. the new reject tests); os-cfg riscv64 check keystored/statefsd/selftest-client clean;
  structure-gate (keystored os_stub 1225 → 1224), dep-gate, fmt, clippy all PASS. Marker
  strings unchanged. Awaiting QEMU re-run (main session).
- Steps 3 (settingsd client migration — already in tree, uncommitted) and 4
  (keystored/updated `put_authenticated` adoption) remain.

### 2026-08-15 — plan step 4 landed (keystored/updated envelope adoption); only the QEMU re-run remains

keystored and updated now write Integrity-class envelopes for their own keys — the
first-party migration window is closed (statefsd's accept-and-audit path now only covers
legacy raw values on the medium and third-party writers). All behind existing markers; no
new marker strings.

- **Shared helper `statefs::writer`** (new, cfg-free): `open_stored` (envelope-or-legacy
  classification, deterministic `Corrupted` on magic-bearing malformed bytes — never a
  legacy fallback that could mask tampering, never a panic), `next_seq`
  (read-modify-write discipline: first write / legacy = seq 1, else last-seen + 1,
  saturating), `seal_integrity` (alg = none — chicken-egg rule holds: no MAC key, no new
  IPC on the device-key bootstrap path).
- **keystored**: new cfg-free `state_record` (subject `keystored`, purposes
  `device-key` / `scoped-kv`; device-key payload must be exactly 32 bytes) + new
  `store_os` (KeyStore/StatefsStore moved out of the LOC-capped os_stub.rs; every
  `/state/keystore/*` put — device.signing and the scoped KV shim — is a
  read-modify-write seal with one bounded retry on a rollback race; reads accept
  envelope v1 AND legacy raw). os_stub.rs 1224 → 996 LOC.
- **updated**: bootctl codec moved cfg-free to `bootctl_state` (subject `updated`,
  purpose `bootctl`) + `seal_bootctl`/`open_bootctl`; `persist_bootctrl_state` is the
  same RMW loop (get → seq+1 → put → sync, loud labeled errors kept). os_lite.rs
  965 → 918 LOC.
- **statefsd** (compatibility fix, noted): `envelope_mac_key` unwraps the now-enveloped
  `device.signing` record via `statefs::writer::open_stored` before deriving the MAC key
  (raw 32-byte legacy seeds still work) — without this, Authenticated prefixes would have
  failed closed forever once keystored envelopes its record. Hardening logic untouched.
- **Host proofs**: `cargo test -p statefs -p statefsd -p keystored -p updated` = 128
  passed / 0 failed, incl. new `keystored/tests/state_record_contract.rs` (5) and
  `updated/tests/bootctl_envelope.rs` (4): envelope roundtrip, legacy-raw migration read,
  `test_reject_stale_seq_*_rewrite` (server SeqTracker contract vs. the client
  discipline), `test_reject_malformed_envelope_deterministic_no_panic`. statefs `writer`
  unit tests (4) cover classification + seq discipline.
- **Gates**: os-cfg riscv64 check (keystored/updated/statefsd/selftest-client, os-lite)
  zero warnings; host-cfg check zero warnings; structure-gate, dep-gate, clippy, pinned
  fmt all PASS.
- **Remaining for DoD (main session)**: one QEMU ladder run proving the four registered
  markers plus the existing `SELFTEST: device key persist ok` / `SELFTEST: statefs
  persist ok` / bootctl markers with envelopes on (also expect NO
  `statefsd: envelope migration accept` lines for keystored/updated fresh writes).

#### 2026-08-15 — QEMU run 2 regressions fixed (bootctl probe + delete/re-put seq)

Run `headless--2026-08-15T12-46-51` was RED on step 4; both root causes confirmed and fixed:

1. **`SELFTEST: bootctl persist FAIL`** — the probe
   (`selftest-client/src/os_lite/services/bootctl.rs`) GETs `/state/boot/bootctl.v1` raw
   and asserted `len == 6 && bytes[0] == 1`; the record is now an envelope. Fix: unwrap
   via `statefs::writer::open_stored` first (legacy raw still passes; malformed fails
   deterministically). Marker strings untouched. Host mirror:
   `updated/tests/bootctl_envelope.rs::test_bootctl_selftest_probe_shape`.
2. **241× `statefsd: envelope deny …/6b31 status=10`** — bringup's `keystored_ping` does
   PUT k1 → GET → DEL → GET-miss (tracker high-water = 1, value deleted); the policy
   phase re-resolves keystored, which re-runs the ping: the RMW learned seq from the
   (missing) stored value → sealed seq 1 → RollbackDetected, and
   `resolve_keystored_client` retries up to 128× (the deny flood; enforcement itself was
   correct). Fix in the writer's seq discipline: new bounded
   `statefs::writer::SeqCache` — keystored's `StatefsStore` now uses
   `seq = max(stored seq, last seq written) + 1` and records every accepted write;
   DELETE never evicts. statefsd's rollback enforcement untouched. Host proofs:
   `test_scoped_kv_put_delete_reput_seq_progression` (keystored) +
   `test_seq_cache_survives_delete_then_reput` / `test_seq_cache_takes_max_of_stored_and_cached`
   (statefs). All suites green (132 tests), os/host-cfg zero warnings, structure/dep
   gates + clippy + pinned fmt PASS. QEMU re-run still pending (main session).

## Closure (2026-08-15)

QEMU proof complete: `build/logs/headless--2026-08-15T13-48-44/uart.log` — all four
markers present (`statefsd: write hardening on (auth-envelope)`,
`SELFTEST: statefs auth put ok` / `tamper deny ok` / `rollback deny ok`), plus
`SELFTEST: bootctl persist ok`, `device key pubkey ok`, `device key persist ok`,
`keystored sign denied ok`; zero `envelope deny` under `/state/keystore/`, zero
migration-accept lines (keystored/updated write envelopes natively), zero FAIL
variants. Host: 132 tests green across statefs/statefsd/keystored/updated.

Two hardening by-products shipped with the closure:

- `scripts/qemu-test.sh` now carries a TASK-0025 fake-green guard: once
  `write hardening on (auth-envelope)` appears, the three hardening ok-markers are
  mandatory and their FAIL variants fatal (the proof-manifest phase walker counts a
  FAIL variant as "marker seen", so this was a real enforcement hole — observed in
  the 12:56 run, which exited 0 despite three FAIL markers).
- `selftest-client` device-key probe: bounded re-recv (≤4 waits) on keygen — the
  enveloped RMW made keygen slower than one recv budget; an abandoned recv left the
  late response queued and poisoned every subsequent probe on the channel (the 12:56
  cascade). Latent before this task: `SELFTEST: device key pubkey ok` had been
  silently absent from earlier "green" runs; it now appears for the first time.

Residual scope tracked elsewhere: multi-op atomicity/compaction/fsck → TASK-0026;
value encryption → TASK-0027; quotas → TASK-0133; full-journal rollback anchor →
TASK-0289/boot chain (documented boundary).
