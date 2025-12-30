---
title: TASK-0192 Privacy v2b (OS/QEMU): policy usage telemetry pipeline + privacy dashboard v2 (timeline/stats/revoke/export) + nx-privacy + selftests/docs
status: Draft
owner: @ui
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Privacy v2 host-first telemetry/export: tasks/TASK-0191-privacy-v2a-host-policy-usage-events-telemetry-export.md
  - Policy v1.1 OS UI owner (Privacy Dashboard baseline): tasks/TASK-0168-policy-v1_1-os-runtime-prompts-privacy-dashboard-cli.md
  - Policy v1.1 core semantics: tasks/TASK-0167-policy-v1_1-host-scoped-grants-expiry-enumeration.md
  - Audit sink direction (logd): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Persistence substrate (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Policy caps/adapters: tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We already have:

- a `privacyd` service name reserved for camera/mic/screen indicators (`TASK-0103`),
- a Privacy Dashboard v1.1 in Settings owned by `TASK-0168`.

Privacy v2 adds a **usage timeline** and **per-cap stats** derived from policy decisions, plus deterministic export.
To avoid name collision and authority drift, introduce a small aggregator service named **`privacytelemd`**
(or similar) that reads from the chosen audit sink and serves query/revoke/export APIs.

## Goal

Deliver:

1. Policy usage emission (OS wiring):
   - policyd emits structured “Use” events for `require()` and grant lifecycle decisions
   - sink alignment:
     - preferred: structured records into `logd` once available
     - fallback: deterministic UART markers only (explicitly labeled; no fake “timeline”)
2. Aggregator service `privacytelemd` (new):
   - reads the audit stream (logd query or on-disk JSONL when available)
   - maintains a bounded cache (quota-enforced)
   - APIs:
     - window(since/until, filters, limit)
     - topCaps(appId, limit)
     - revoke(appId, cap) → forwards to policyd revoke
     - export(since/until, outPath) → writes NDAP v1 deterministically under `/state`
     - purge(olderThan)
   - markers (rate-limited):
     - `privacytelemd: ready`
     - `privacy: window items=<n>`
     - `privacy: revoke app=<a> cap=<c>`
     - `privacy: export bytes=<n>`
3. Settings UI: Privacy Dashboard v2 (extends `TASK-0168` page):
   - tabs:
     - Timeline (virtualized list, filters)
     - App Insights (top caps per app, quick revoke)
     - Manage (existing grants + export button)
   - must show “audit unavailable” when sink/cache isn’t available (no fake data)
   - markers:
     - `ui: privacy timeline open`
     - `ui: privacy insights app=<id>`
     - `ui: privacy export out=<path>`
4. CLI `nx-privacy`:
   - `top`, `window`, `revoke`, `export`, `purge`
   - stable time parsing (`now`, `1h`, `7d`) using injected clock for tests
5. Policy caps + quotas:
   - caps:
     - `privacy.read`, `privacy.export`, `privacy.revoke` (system-only default)
   - quotas:
     - audit channel cap_use size/rotation (only real once `/state` exists)
     - privacy cache soft/hard
6. OS selftests (bounded):
   - generate a few allow/deny decisions via policyd
   - query window and verify items > 0 deterministically
   - revoke a cap and verify subsequent require denies
   - export NDAP and verify file exists + size > 0
   - markers:
     - `SELFTEST: privacy timeline ok`
     - `SELFTEST: privacy revoke ok`
     - `SELFTEST: privacy export ok`

## Non-Goals

- Kernel changes.
- Introducing a competing `auditd` authority (sink must align with logd direction).

## Constraints / invariants (hard requirements)

- `/state` gating:
  - retention and export are only real if `/state` exists (`TASK-0009`)
  - without `/state`, exports must be disabled and selftests must not claim export ok
- Determinism: stable ordering, stable export bytes for fixed inputs.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **RED (audit sink dependency)**:
  - Real timeline/stats require a real sink. If `logd` is not implemented yet, v2b must show “audit unavailable”
    and selftests must explicitly skip/placeholder timeline instead of claiming ok.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p privacy_v2_host -- --nocapture` (from v2a)

- **Proof (QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=185s ./scripts/qemu-test.sh`
  - Required markers:
    - `SELFTEST: privacy timeline ok`
    - `SELFTEST: privacy revoke ok`
    - `SELFTEST: privacy export ok`

## Touched paths (allowlist)

- `source/services/policyd/` (usage event emission)
- `source/services/privacytelemd/` (new)
- `userspace/systemui/dsl/pages/settings/` (extend privacy dashboard page)
- `tools/nx-privacy/` (new)
- `source/apps/selftest-client/`
- `schemas/privacy.schema.json` (new/extend)
- `docs/privacy/`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. privacytelemd core + cache + window/topCaps/revoke/export APIs + markers
2. Settings Privacy Dashboard v2 UI + CLI
3. selftests + docs + marker contract update

## Acceptance criteria (behavioral)

- In QEMU (when unblocked by logd + /state), privacy timeline/stats/revoke/export flows are proven via deterministic markers.

