---
title: TASK-0141 Crash v1 (offline): crash notifications + export/redaction surface over .nxcd.zst (no new formats) + tests/markers/docs
status: Draft
owner: @reliability
created: 2025-12-25
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Crashdump v2b OS pipeline (crashd + .nxcd.zst): tasks/TASK-0049-crashdump-v2b-os-crashd-retention-correlation-policy.md
  - Crashdump v2a host tooling (`nx crash`): tasks/TASK-0048-crashdump-v2a-host-pipeline-nxsym-nx-crash.md
  - Notifications v2 (for crash notifications): tasks/TASK-0069-ui-v8a-notifications-v2-actions-inline-reply.md
  - Policy capability gates (diagnostics.export): tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Share v2 (optional export via chooser): tasks/TASK-0126-share-v2a-intentsd-registry-dispatch-policy-host.md
  - Persistence (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - logd (recent logs pull): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
---

## Context

Your prompt proposes a local/offline crash pipeline with:

- in-process capture,
- an aggregator service,
- notifications,
- a bundler/exporter service producing a zip `.ncr`.

Repo reality already has the crash pipeline direction:

- OS artifacts: `.nxcd.zst` + `.report.txt` under `/state/crash/...` (`TASK-0049`)
- host tools: `nx crash ...` and `nxsym` (`TASK-0048`)

To avoid drift, this task adds **export and notification UX** on top of the existing `.nxcd` artifacts
without introducing a new `.ncr` format or JSON-as-contract.

## Goal

Deliver:

1. Crash notifications (offline):
   - when `crashd` ingests a new crash, emit a Notifications v2 entry:
     - channel: `system/crash`
     - priority: high
     - action: `Open Report` (deep-link into Problem Reporter UI task)
   - lock-screen redaction follows notifications visibility rules (handled by notifs tasks)
   - markers:
     - `crash: notified id=...`
2. Export/redaction surface:
   - add an API to `crashd` (or a tiny companion service, if needed) to:
     - produce an exportable URI for a **single** crash report as `.nxcd.zst`
     - apply a redaction level (policy-driven) before producing the export artifact
   - export output path:
     - `content://state/crash/exports/<id>.nxcd.zst` (or equivalent)
   - enforce capability gate:
     - `diagnostics.export` required to export crashes not owned by the caller subject
     - default: export disabled unless explicitly allowed by policy (align with `TASK-0049`)
   - markers:
     - `crash: export ok id=... uri=...`
3. Retention interaction:
   - export artifacts are accounted to the crash retention budget (or a separate bounded budget)
   - eviction must be deterministic and documented

## Non-Goals

- Kernel changes.
- Introducing a new zip bundle format (`.ncr`). We reuse `.nxcd.zst`.
- Multi-report bundling (can be a follow-up once single-report export is stable).
- Full UI (handled by `TASK-0142`).

Related follow-up:

- Deterministic “bugreport bundles” and `nx diagnose` orchestration are planned in `TASK-0227`
  (reuses `.nxcd.zst`; no new crash formats).

## Constraints / invariants (hard requirements)

- Offline-only: no network egress.
- Deterministic markers and deterministic redaction.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

Deterministic host tests (suggested: `tests/crash_export_host/`):

- given a fixture `.nxcd`, export produces a stable redacted output (golden hash)
- policy gate denies export without `diagnostics.export` deterministically
- notification payload fields stable (when notifs v2 host tests are present)

### Proof (OS/QEMU) — gated

UART markers:

- `crashd: ready`
- `crash: notified id=...`
- `SELFTEST: crash export ok`

## Touched paths (allowlist)

- `source/services/crashd/` (extend export + redaction + notification emit)
- `source/services/notifd/` (channel registration if needed)
- `source/apps/selftest-client/` (gated marker)
- `docs/reliability/crashdump-v2.md` (OS section update)
- `docs/telemetry/` (new or reuse reliability docs)

## Plan (small PRs)

1. define crash notification channel and emit on ingest
2. implement export API for single crash + policy gate + markers
3. host tests + docs; OS marker wiring once deps exist
