---
title: TASK-0204 IME v2.1b (OS/QEMU): SecureFS-backed personalization store + caps/quotas + UI (badges/forget/export/import/toggle) + nx-ime extensions + selftests/docs
status: Draft
owner: @ui
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - IME v2.1 host core (rank/train/export): tasks/TASK-0203-ime-v2_1a-host-adaptive-ranking-training-export.md
  - IME v2 engines + dict baseline: tasks/TASK-0149-ime-v2-part2-cjk-engines-userdict.md
  - Candidate popup + OSK + OS proofs: tasks/TASK-0150-ime-v2-part2b-candidate-popup-osk-cjk-os-proofs.md
  - SecureFS overlay (state:/secure): tasks/TASK-0183-encryption-at-rest-v1b-os-securefsd-unlock-ui-migration-cli-selftests-docs.md
  - Policy cap matrix baseline: tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Policy v1.1 OS UI/CLI direction: tasks/TASK-0168-policy-v1_1-os-runtime-prompts-privacy-dashboard-cli.md
  - Persistence substrate (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

IME v2.1a defines deterministic ranking/training/export semantics host-first.
This task wires persistence and user-facing controls in OS:

- store personalization data under SecureFS (`state:/secure/ime/...`),
- add caps/quotas and enforce “system-only raw access”,
- integrate badges/forget/toggle/export/import in SystemUI and `nx-ime`,
- prove behavior in QEMU selftests without fake success.

## Goal

Deliver:

1. SecureFS-backed IME personalization store:
   - storage root: `state:/secure/ime/`
   - per-locale files (deterministic naming), e.g.:
     - `state:/secure/ime/<lang>/user_dict.jsonl`
     - `state:/secure/ime/<lang>/ctx_bigram.jsonl`
     - `state:/secure/ime/meta.json`
   - quotas enforced (rows/bytes) with deterministic eviction policy (as per v2.1a)
   - markers:
     - `imestore: open ok lang=<...>`
     - `imestore: upsert dict cand="..." freq=<n>`
     - `imestore: bigram prev="..." cand="..." freq=<n>`
2. `imed` integration:
   - candidate assembly passes features to `ime_ranker`
   - training on commit updates the SecureFS store (when personalization enabled)
   - recency uses deterministic bucketization (no raw timestamps in export)
   - markers:
     - `imerank: rerank n=<k> top="..." score=<s>`
3. Policy + schema + quotas:
   - config schema for weights/limits/export cap
   - caps:
     - `ime.train`, `ime.export`, `ime.import`
   - third-party apps must not read raw personalization data (only IME service accesses the store)
4. SystemUI:
   - candidate popup shows “personalized” badge on candidates influenced by personal features
   - “Forget this suggestion” action (deterministic rule)
   - Settings → Keyboard & Input → Personalization:
     - toggle personalization on/off
     - export/import UI (bounded)
   - markers:
     - `ime-ui: forget cand="..."`
     - `ime-ui: export out=<...>`
     - `ime-ui: import merged=<n>`
5. Export/import:
   - export path:
     - `state:/exports/ime-profile.ndjson` (requires `/state`; must be disabled without it)
   - deterministic sort order and merge rules
6. CLI `nx-ime` extensions (host tool):
   - status/train/forget/export/import/stats
   - NOTE: QEMU selftests must not require running host tools inside QEMU
7. OS selftests (bounded):
   - JP: commit a non-top candidate twice → becomes top next time:
     - `SELFTEST: ime rank adapt ok`
   - bigram boost:
     - `SELFTEST: ime bigram ok`
   - export/import roundtrip:
     - `SELFTEST: ime export/import ok`
   - toggle personalization off → order reverts:
     - `SELFTEST: ime personalization toggle ok`

## Non-Goals

- Online personalization sync.
- Large ML language models.
- Claiming SecureFS is real unless `securefsd` is unblocked (see gates).

## Constraints / invariants (hard requirements)

- **Gating**:
  - SecureFS-backed persistence requires `TASK-0183` and `/state` (`TASK-0009`).
  - If SecureFS is not available, v2.1b must be explicit `stub/placeholder` (no “ok” markers).
- No fake success: “rank adapt ok” must verify ordering changes via IME snapshots, not log-greps.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p ime_v2_1_host -- --nocapture` (from v2.1a)

- **Proof (QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=185s ./scripts/qemu-test.sh`
  - Required markers:
    - `SELFTEST: ime rank adapt ok`
    - `SELFTEST: ime bigram ok`
    - `SELFTEST: ime export/import ok`
    - `SELFTEST: ime personalization toggle ok`

## Touched paths (allowlist)

- `source/services/imed/` (ranker + store wiring)
- SystemUI IME overlay + Settings pages
- `tools/nx-ime/` (extensions)
- `schemas/ime_v2_1.schema.json`
- `source/apps/selftest-client/`
- `docs/ime/` + `docs/tools/nx-ime.md`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. schema + caps + store wiring to SecureFS
2. SystemUI actions (badge/forget/toggle/export/import)
3. nx-ime extensions
4. OS selftests + docs + postflight wrapper (delegating)

## Acceptance criteria (behavioral)

- In QEMU (when unblocked by SecureFS + /state), IME personalization adapts deterministically and export/import/toggle are proven by selftest markers.

