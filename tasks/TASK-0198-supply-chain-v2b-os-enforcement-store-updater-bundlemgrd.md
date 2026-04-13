---
title: TASK-0198 Supply-Chain hardening v2b (OS/QEMU): enforce sigchain/translog/SBOM/provenance + anti-downgrade in storemgrd/updated/bundlemgrd + selftests/docs
status: Draft
owner: @security
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Supply-chain v2 host core: tasks/TASK-0197-supply-chain-v2a-host-sigchain-translog-sbom-provenance.md
  - Supply-chain v1 baseline: tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md
  - Trust store unification: tasks/TASK-0160-identity-keystore-v1_1-os-attestd-trust-unification-selftests.md
  - Store v1 (consumer): tasks/TASK-0181-store-v1b-os-storefront-ui-selftests-policy-docs.md
  - Updated v2 (consumer): tasks/TASK-0179-updated-v2-offline-feed-delta-health-rollback.md
  - Packages install path (bundlemgrd): tasks/TASK-0130-packages-v1b-bundlemgrd-install-upgrade-uninstall-trust.md
  - Persistence substrate (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

This task wires supply-chain v2 verification into real OS install/update paths:

- Store installs/updates,
- Updater staging/apply,
- and bundlemgrd install pipeline.

Everything remains offline by default (pkg:// fixtures). Enforcement must be deterministic and must not introduce partial installs.

## Goal

Deliver:

1. OS `translogd` service (if v2 requires it at runtime):
   - provides inclusion checks for local fixtures (offline)
   - `/state` persistence is gated; without `/state`, translog can be RAM-only and must not claim persistence
2. Enforcement integration:
   - `storemgrd`:
     - requires envelope present for store feed artifacts (if configured `sbom_required=true`)
     - verifies sigchain + translog inclusion + SBOM hash before calling bundlemgrd install
     - enforces anti-downgrade vs installed version (SemVer + optional build counter) deterministically
   - `updated`:
     - verifies envelope for OTA manifest/artifacts before writing to inactive slot
     - anti-downgrade: refuse older version/build than current unless dev-mode allows
   - `bundlemgrd`:
     - optionally verifies envelope (when provided) and records verification report next to installed bundle metadata
3. Provenance recording:
   - record provenance in an append-only store under `/state` (gated)
   - show a minimal “Verified” surface to UI (optional: Storefront details)
4. Deterministic error reporting:
   - failures must return stable errors (“sigchain missing”, “translog inclusion missing”, “sbom hash mismatch”, “downgrade denied”)
5. OS selftests (bounded):
   - install good store fixture → ok
   - install tampered envelope → deny
   - apply OTA good fixture → ok
   - apply OTA downgrade fixture → deny
   - markers:
     - `SELFTEST: supply store install ok`
     - `SELFTEST: supply store tamper deny ok`
     - `SELFTEST: supply ota ok`
     - `SELFTEST: supply ota downgrade deny ok`
6. Docs:
   - enforcement flow for store and updater
   - transparency log threat model (offline)
   - SBOM/provenance surfaces

## Non-Goals

- Kernel changes.
- Global/online transparency log.
- “Perfect” rollback protection without boot-chain proof (A/B booted slot truth remains `TASK-0037`).

## Constraints / invariants (hard requirements)

- No partial installs: verify-before-commit everywhere.
- `/state` gating: provenance and persisted translog require `TASK-0009`.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **RED (anti-rollback vs boot-chain)**:
  - We can enforce anti-downgrade at install/apply time based on version/build counters, but we cannot prove “booted slot” rollback without boot-chain integration.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p supply_hardening_v2_host -- --nocapture` (from v2a)

- **Proof (QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=210s ./scripts/qemu-test.sh`
  - Required markers:
    - `SELFTEST: supply store install ok`
    - `SELFTEST: supply store tamper deny ok`
    - `SELFTEST: supply ota ok`
    - `SELFTEST: supply ota downgrade deny ok`

## Touched paths (allowlist)

- `source/services/translogd/` (new; if runtime service is used)
- `source/services/storemgrd/` (enforcement)
- `source/services/updated/` (enforcement)
- `source/services/bundlemgrd/` (optional enforcement/recording)
- `source/apps/selftest-client/`
- `pkg://store/` + `pkg://updates/` fixtures (envelopes/sboms)
- `docs/supply/` + `docs/store/` + `docs/update/`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. translogd runtime presence decision + minimal service if needed
2. storemgrd enforcement path + tests/selftest markers
3. updated enforcement path + tests/selftest markers
4. docs + marker contract updates

## Acceptance criteria (behavioral)

- In QEMU, store/updater accept valid fixtures and deterministically reject tampered/downgrade fixtures with no partial writes.
