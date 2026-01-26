---
title: TASK-0162 Backup/Restore v1b (OS/QEMU): backupd service + device-bound wrapping + policy caps + Settings/CLI + selftests/postflight + docs
status: Draft
owner: @runtime
created: 2025-12-26
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - NBK v1 engine (host-first): tasks/TASK-0161-backup-restore-v1a-host-nbk-format-pack-verify-restore.md
  - Persistence substrate (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Prefs substrate: tasks/TASK-0072-ui-v9b-prefsd-settings-panels-quick-settings.md
  - Policy grants substrate: tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Recents substrate (optional): tasks/TASK-0082-ui-v11b-thumbnailer-recents.md
  - Search v2 backend (optional index backup): tasks/TASK-0154-search-v2-backend-os-persistence-selftests-postflight-docs.md
  - Keystore v1.1 OS wiring (seal/unseal): tasks/TASK-0160-identity-keystore-v1_1-os-attestd-trust-unification-selftests.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

With the NBK v1 format and host-first engine proven (`TASK-0161`), we can wire an OS-facing slice:

- `backupd` service that creates/verifies/restores NBK bundles to `/state`,
- device-bound wrapping using `keystored.seal/unseal`,
- policy caps, quotas, retention,
- Settings UI and `nx-backup` CLI,
- bounded OS selftests and a delegating postflight.

## Goal

Deliver:

1. `backupd` service API (Cap’n Proto):
   - plan/dryRun/create/list/verify/delete/restoreDryRun/restore/stats
   - storage: `state:/backup/bundles/<id>.nbk`
   - deterministic packing order and deterministic bundle IDs (seeded)
   - markers (rate-limited):
     - `backupd: ready`
     - `backup: dryrun files=<n> bytes=<n>`
     - `backup: create id=<id> bytes=<n>`
     - `backup: verify id=<id> ok`
     - `backup: restore id=<id> ok`
     - `backup: evict n=<n>`
2. Sources (offline, deterministic, allowlisted):
   - apps data: allowlist under `state:/apps/<appId>/...` with explicit excludes (no caches/logs)
   - system prefs: from prefsd/prefs store (or deterministic fixture if not present yet)
   - policy grants: from policy storage (or deterministic fixture if not present yet)
   - recents: optional, bounded
   - search index: optional, only if search backend exists and index is bounded
3. Device-bound wrapping:
   - wrap sensitive blobs via `keystored.seal` using a system-owned key/purpose (e.g. `backup-wrap`)
   - on restore, `unseal` must fail deterministically on device mismatch; no plaintext private key material in backups
4. Policy caps + quotas + retention:
   - caps:
     - `backup.create`, `backup.restore`, `backup.read`, `backup.delete`
   - quotas:
     - per-bundle size caps (documented)
     - total backup storage soft/hard limits
   - deterministic eviction policy (oldest first)
   - audit markers/events for create/restore/delete (sink may be UART until logd exists)
5. UI + tooling:
   - Settings → Backup & Restore DSL page
   - `nx-backup` CLI
6. OS selftests (bounded, QEMU-safe):
   - `SELFTEST: backup v1 create ok`
   - `SELFTEST: backup v1 verify ok`
   - `SELFTEST: backup v1 restore ok`
   - `SELFTEST: backup v1 retention ok`
7. Docs + postflight:
   - docs: overview/usage/security/testing
   - postflight delegates to canonical proofs:
     - host tests (`backup_restore_v1_host`)
     - QEMU marker contract (`scripts/qemu-test.sh`)

## Non-Goals

- Kernel changes.
- Network/cloud backups.
- Passphrase export mode (future v1.1).

## Constraints / invariants (hard requirements)

- No fake success:
  - if `/state` is unavailable, `backupd` must not claim persistence; must emit explicit `stub/placeholder` markers
  - if `keystored.seal/unseal` is unavailable, wrapping must be explicit `stub/placeholder` and backups must refuse “secure” mode
- Determinism: stable ordering; injected clocks; stable IDs.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **RED (requires `/state`)**:
  - real backup bundles live under `/state`. This task is OS-gated on `TASK-0009`.

- **RED (requires keystored seal/unseal)**:
  - device-bound wrapping requires keystored v1.1 `seal/unseal` semantics (`TASK-0160`).

- **YELLOW (source availability)**:
  - prefs/policy/recents/search index sources may not be present yet. Any fallback must be explicit and deterministic.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p backup_restore_v1_host -- --nocapture`

- **Proof (QEMU)**:
  - Command(s):
    - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=195s ./scripts/qemu-test.sh`
  - Required markers (to be added to `scripts/qemu-test.sh` expected list):
    - `backupd: ready`
    - `SELFTEST: backup v1 create ok`
    - `SELFTEST: backup v1 verify ok`
    - `SELFTEST: backup v1 restore ok`
    - `SELFTEST: backup v1 retention ok`

## Touched paths (allowlist)

- `source/services/backupd/` (new)
- `tools/nx-backup/` (new)
- SystemUI Settings DSL pages (backup/restore)
- `source/apps/selftest-client/`
- `scripts/qemu-test.sh` (marker contract update)
- `tools/postflight-backup-v1.sh` (delegates)
- `docs/backup/` (new)
- `docs/dev/ui/testing.md` (link)

## Plan (small PRs)

1. backupd API + deterministic bundle creation/verify/restore + markers
2. device-bound wrapping integration via keystored + policy caps
3. Settings UI + nx-backup CLI
4. selftests + marker contract + docs + postflight

## Acceptance criteria (behavioral)

- Host tests prove deterministic NBK behavior; OS/QEMU selftests prove create/verify/restore/retention markers.
- Any missing dependencies (`/state`, keystored seal/unseal) are handled explicitly without “ok” markers.
