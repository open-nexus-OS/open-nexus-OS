---
title: TASK-0142 Problem Reporter UI v1 (offline): list/open/delete/export crash reports + deep-link from notifications + host snapshots/tests + OS markers
status: Draft
owner: @ui
created: 2025-12-25
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Crash export/notify surface: tasks/TASK-0141-crash-v1-export-redaction-notify.md
  - Crashdump v2b OS pipeline: tasks/TASK-0049-crashdump-v2b-os-crashd-retention-correlation-policy.md
  - SystemUI→DSL baseline: tasks/TASK-0121-systemui-dsl-migration-phase2a-settings-notifs-host.md
  - Share v2 (optional export destination): tasks/TASK-0126-share-v2a-intentsd-registry-dispatch-policy-host.md
---

## Context

Once `crashd` stores crash artifacts under `/state/crash/...`, users need a local/offline UI to:

- view recent crashes,
- inspect symbolized stack traces and bounded metadata,
- delete reports,
- export a report (policy-gated).

This is strictly local/offline and deterministic.

## Goal

Deliver `userspace/apps/problem-reporter` (or SystemUI panel) with:

1. List view:
   - latest crash reports (app icon/name, timestamp, reason)
   - actions: open, delete
2. Detail view:
   - symbolized stack (if available), registers summary, fault address, bounded log preview
   - user note/annotation (stored as separate metadata next to report, bounded)
3. Export flow:
   - export a single report via `crashd.export(...)` (or reporterd if introduced later)
   - optionally invoke Share v2 chooser to send the exported URI to Save/Files/etc.
4. Deep-link:
   - “Open Report” notification action opens the correct report id
5. Markers:
   - `problem-reporter: open id=...`
   - `problem-reporter: export uri=...`

## Non-Goals

- Kernel changes.
- Multi-report bundling export (follow-up).
- Remote telemetry upload (never in v1).

## Constraints / invariants (hard requirements)

- Deterministic snapshots for host tests (stable layout and ordering).
- A11y labels/roles for lists/buttons.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

New deterministic host tests (suggested: `tests/problem_reporter_host/`):

- list renders stable ordering from a deterministic crashd mock
- detail view renders stable stack/log preview snapshots (light/dark/HC)
- export button calls mock and returns expected URI

### Proof (OS/QEMU) — gated

UART markers:

- `SELFTEST: crash v1 ui ok` (or `problem-reporter: open id=...`)
- `SELFTEST: crash export ok`

## Touched paths (allowlist)

- `userspace/apps/problem-reporter/` (new)
- `userspace/systemui/dsl_bridge/` (extend for crashd calls)
- `tests/`
- `docs/telemetry/problem-reporter.md` (new)

## Plan (small PRs)

1. UI skeleton (list/detail) + a11y + markers
2. export wiring + notification deep-link
3. host snapshots/tests + docs; OS markers once deps exist
