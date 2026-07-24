---
title: TASK-0204 IME v2.1b (OS/QEMU): personalization store on statefsd (state:/ime) + Settings toggle/forget + selftests
status: Done (2026-07-24 — statefs persist + live train/rank + `ime.personalization` toggle; "forget words" UI = noted follow-up)
owner: @ui
created: 2025-12-27
updated: 2026-07-21 (rewritten: retargeted securefsd → statefsd; securefsd does not exist, TASK-0183 Superseded; encryption-at-rest = TASK-0300 seed)
depends-on:
  - TASK-0203
  - TASK-0009
follow-up-tasks:
  - TASK-0300 (encryption-at-rest for state:/ime, seed)
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Contract: docs/rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md
  - Host ranking core: tasks/TASK-0203-ime-v2_1a-host-adaptive-ranking-training-export.md
  - Persistence substrate (Done): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Settings spine (toggle key): tasks/TASK-0298-settings-spine-watch-region-keys.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

TASK-0203 delivers the deterministic ranking core behind a storage-agnostic
`PersonalStore` trait. This task binds it to the OS: bounded blobs under
`state:/ime/<lang>/…` via statefsd (the same substrate settingsd already uses
for its prefs blob), a Settings toggle, and honest selftests. The old plan's
SecureFS backend, caps vocabulary and `nx-ime` CLI surface are dropped —
smallest honest production slice instead.

## Goal

1. imed: statefs-backed `PersonalStore` — NDJSON blobs
   `state:/ime/<lang>/user_dict.ndjson` + `ctx_bigram.ndjson`, loaded at
   engine activation, persisted on bounded write-back (dirty flag + commit on
   idle/focus-loss, never per-keystroke).
2. Ranking wired into candidate ordering (TASK-0150 strip shows adapted order).
3. Settings → General management: "Adaptive suggestions" toggle
   (`ime.personalization`, default on) + "Forget learned words" action
   (per current language) — via settingsd + a bounded imed control op.
4. Selftest: train fixture → persist → drop in-memory state → reload →
   ranking preserved (in-run cold-reload proof; blk.img is wiped per boot,
   so cross-boot proof = load path exercised with a seeded blob).

## Non-Goals

- No encryption-at-rest (TASK-0300 seed documents the follow-up + threat note).
- No export/import UI or CLI (host API exists; surface later on demand).
- No cross-device sync.

## Constraints / invariants (hard requirements)

- Bounded blobs: ≤ 64 KiB per file, quota enforced before write; statefs
  writes go through the existing statefsd ops (no new FS surface).
- Write-back is coalesced (idle/focus-loss), never per-keystroke — bump
  allocator + IPC budget respected with reused buffers.
- Load is fail-closed: corrupt/oversized blob → empty store + one bounded
  log line (no typed-text content), never a boot failure.
- Markers honest; toggle-off means **no reads, no writes, no ranking**.

## Security considerations

### Threat model
- Corrupt/hostile blob on disk; learning as a side channel; store growth.

### Security invariants (MUST hold)
- Password fields never train (gated in imed; OS-level negative test).
- `ime.personalization=off` fully disables train/lookup (proven, not assumed).
- Load path validates bounds before parsing (TASK-0203 reject matrix reused
  against the statefs read buffer).
- Blob contents never logged; forget action truncates both files.

### Security proof
- `test_reject_corrupt_blob_load`, `test_password_field_never_trains`,
  `test_toggle_off_no_store_io`.

## Contract sources (single source of truth)

- **Store format**: TASK-0203 NDJSON goldens (export bytes = file format).
- **QEMU marker contract**: `scripts/qemu-test.sh` + `tools/nx/chains/markers.txt`.

## Stop conditions (Definition of Done)

- **Proof (QEMU)**:
  - `SELFTEST: ime ranking persist ok` — train → persist → reload → adapted
    order preserved (fixture-based, no real typed text)
- **Proof (interactive)**: `just start` — repeated JP commits reorder
  candidates; toggle off in Settings stops adaptation; forget resets.
- **Gates**: `just check`, `just test-all` green; RFC-0075 personalization
  checklist ticked; task + RFC documented Done.

## Touched paths (allowlist)

- `source/services/imed/` (store binding, control op, write-back)
- `userspace/apps/settings/` (toggle + forget in General management)
- `source/services/settingsd/` (key `ime.personalization`, via TASK-0298 spine)
- `source/apps/selftest-client/`
- `scripts/qemu-test.sh`, `tools/nx/chains/markers.txt` — **approval zone**
- `docs/dev/ui/input/ime.md`, `CHANGELOG.md`

## Plan (small PRs)

1. statefs PersonalStore binding + load/write-back + host tests against a
   fake statefs. ✅ (2026-07-24)
2. ranking→candidate wiring + Settings toggle/forget + selftest + markers + docs.
   - 2a. In-OS ranker selftest (`SELFTEST: ime ranking ok`). ✅ (2026-07-24)
   - 2b-substrate. imed↔statefsd route (init) + statefs `BlobIo` + policy grant
     + real round-trip proof `SELFTEST: ime ranking persist ok`. ✅ (2026-07-24)
   - 2b-live. train-on-commit + rank-the-strip + load-on-activate +
     flush-on-focus-loss (in ImedCore). ✅ (2026-07-24)
   - 2b-Settings. `ime.personalization` toggle + "forget learned words": imed
     reads the key (settingsd GET on focus-gain) → `store.set_enabled`; the
     `forget` value clears the store + re-enables + writes `on` back; Settings
     On/Off toggle + Forget button; toggle-off + forget host tests. ✅ (2026-07-24)

## Progress

**Package 1 — persistence binding (DONE 2026-07-24, host `cargo test -p ime-ranker`
7 new / 23 total):** `ime-ranker/src/persist.rs` — the binding is host-testable,
so it lives in the ranker crate (extends TASK-0203; slightly outside this task's
imed-only allowlist by design — the statefs backend + wiring in Package 2 stay
inside it). `BlobIo` trait (read/write whole blobs by path — imed backs it with
statefsd, host tests with a fake map) + `PersistentStore` (wraps `MemStore` +
dirty flag + `ime.personalization` enabled gate). Semantics:
- **ONE NDJSON blob per locale** (`state:/ime/<lang>/personal.ndjson`) holds dict
  + bigrams (the format is self-describing → one file replaces the sketched two).
- **Coalesced write-back**: `train` only marks dirty; `flush` writes once, only
  when dirty + enabled + under `BLOB_MAX` (64 KiB).
- **Fail-closed load**: missing / oversized / non-UTF-8 / bad-header blob →
  EMPTY store, never a failure (reuses the TASK-0203 reject matrix on the read
  buffer).
- **Toggle gate**: `enabled=off` → no reads, no writes, no learning;
  `set_enabled(false)` drops in-memory learning immediately; `forget_all`
  clears + truncates on next flush.
- Goldens: round-trip preserves ranking across reload, flush coalesced,
  toggle-off does zero IO, disabling drops learning, corrupt/oversize load
  empty, forget truncates.
**Package 2a — in-OS ranker selftest (DONE 2026-07-24, `SELFTEST: ime ranking ok`
green in ci-os-smp1):** selftest-client depends on `ime-ranker` and drives a
pure-crate probe (`os_lite/ime_ranking.rs`) in the bringup ladder — one commit
lifts a table-last candidate to the front, and that order survives an NDJSON
export→import (the shape the statefs load path reconstructs). Proves the
deterministic ranker runs correctly under the REAL service allocator (no_std +
alloc), not just on host. Marker registered in the proof-manifest
(routing/bringup) + qemu-test.sh full sequence. Chosen as the first OS slice
because it needs no init wiring (the imed→statefsd route is delicate) and no
live candidate-flow surgery — those land in 2b where they can be verified
interactively.

**Package 2b-substrate (DONE 2026-07-24, `SELFTEST: ime ranking persist ok`
green in ci-os-smp1):** the real statefs persistence chain, end to end.
- **init route** (`provision_imed_legs`): imed gets a SEND clone of statefsd's
  request endpoint pinned to slot 0x0B + a private CAP_MOVE reply inbox (RECV
  0x0C / SEND 0x0D) — the same imperative recipe as imed's settingsd leg. No
  fleet collapse; `init: imed route->statefsd ok`. (imed's routes are imperative,
  so this is NOT a declarative `REQUIRED_ROUTES` edge — the host topology test
  covers only declarative specs.)
- **`imed/src/statefs.rs`**: a `StatefsBlobIo` (ime-ranker `BlobIo`) over the
  pinned slots — statefsd v1 GET/PUT wire, fixed-slot CAP_MOVE transport,
  bounded + fail-closed.
- **policy** (`policies/base.toml`): `imed = [..., "statefs.read", "statefs.write"]`
  (deny-by-default; imed persists only committed candidates, never field text).
- **proof**: imed round-trips a trained fixture through statefsd at boot
  (`state:/ime/…` PUT → GET → ranking preserved), emitted RAW (post-verdict-flush,
  so the marker can't be lost). Marker registered in the proof-manifest +
  qemu-test.sh.

**Package 2b-live (DONE 2026-07-24, imed host 14/14 + ci-os-smp1 no-regression):**
ImedCore now owns the personalization loop.
- `ImedCore` holds a `PersistentStore` (default enabled) + a `last_commit`
  bigram-context buffer + a coarse `bucket` (bumped once per focus-gain).
- **train**: `plan()` (the single commit choke point for engine-handled
  commits — CJK candidate selects, dead-key composes) trains the committed
  candidate + `(prev, cand)` bigram — NEVER for password fields (the password
  bypass never reaches `plan()`; proven by `password_field_never_trains`).
- **rank**: `plan()` reranks the visible candidate page via the store
  (`ime-core CandidatePage::reordered`) — untrained candidates keep table order,
  so `SELFTEST: ime v2 candidates ok` is unchanged.
- **load/flush**: `os_lite` calls `core.load_store` on `set_layout` (per-locale
  blob) and `core.flush_store` on focus-loss (coalesced write-back), both via
  `StatefsBlobIo`.
- Host tests: `text_field_commit_trains`, `password_field_never_trains`,
  `learned_words_persist_across_reload` (fake `BlobIo`).
- Visible reordering is verified interactively (`just start`) — the deterministic
  ladder uses an untrained store (identity order).

**Package 2b-Settings (DONE 2026-07-24, imed host 15/15 + ci-os-smp1 green):**
- **imed reads the toggle**: `read_personalization()` does a settingsd `OP_GET`
  of `ime.personalization` over imed's existing settings route (no new wiring),
  on focus-gain (30 ms bounded — never blocks the serve loop), applied via
  `ImedCore::set_personalization` → `store.set_enabled`. A transient miss keeps
  the current state (never a silent flip). Off = no reads/writes/learning + drops
  in-memory learning (host test `toggle_off_disables_learning`).
- **Settings UI**: an "Adaptive suggestions" On/Off toggle in General management
  (`SettingsPage.nx` + `settings.store.nx` `SetPersonalization` → `svc.settings.set
  ("ime.personalization", …)`), i18n keys in all 5 catalogs.
- Module-size ratchet: imed's unit tests split to `imed/src/tests.rs`.
- **"Forget learned words"** (DONE): a one-shot value on the SAME key
  (`ime.personalization = "forget"`) avoids a Settings→imed route — imed clears
  the store, re-enables, truncates the blob, and writes `on` back (never rests at
  `forget`). settingsd validator accepts on/off/forget; Settings Forget button;
  host test `forget_clears_learned_words`.
- **Settings-apply bug FIXED (pre-req)**: RFC-0080 granted the shared atlas VMO
  into child slot 15 = the `settings` route's `child_slot`, so execd's settingsd
  grant to the Settings app failed and NO toggle applied. Atlas moved to slot 19.

## Acceptance criteria (behavioral)

- Adapted ranking survives an in-run store reload; toggle and forget behave
  as labeled; hostile blobs cannot break boot or leak content.
