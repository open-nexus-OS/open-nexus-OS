---
title: TASK-0204 IME v2.1b (OS/QEMU): personalization store on statefsd (state:/ime) + Settings toggle/forget + selftests
status: Draft
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
   fake statefs.
2. ranking→candidate wiring + Settings toggle/forget + selftest + markers + docs.

## Acceptance criteria (behavioral)

- Adapted ranking survives an in-run store reload; toggle and forget behave
  as labeled; hostile blobs cannot break boot or leak content.
