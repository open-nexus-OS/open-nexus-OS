---
title: TASK-0227 Diagnostics v1 (offline): deterministic bugreport bundles + `nx diagnose` over existing crash/log artifacts (no new formats)
status: Draft
owner: @reliability @devx
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - /state (persistence gate): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - logd (recent logs query): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Crash pipeline (OS): tasks/TASK-0049-crashdump-v2b-os-crashd-retention-correlation-policy.md
  - Crash export/redaction (single report): tasks/TASK-0141-crash-v1-export-redaction-notify.md
  - Host tooling (`nx crash`, `nxsym`, `.nxcd`): tasks/TASK-0048-crashdump-v2a-host-pipeline-nxsym-nx-crash.md
  - DevX CLI base (`nx ...`): tasks/TASK-0045-devx-nx-cli-v1.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need a deterministic, offline “bugreport bundle” that operators and developers can export without
inventing a new crashdump format or duplicating service authority.

This prompt overlaps with existing crash tasks:

- Crash artifacts on OS are already planned as `.nxcd.zst` + `.report.txt` under `/state/crash/...` (`TASK-0049`).
- Export/redaction and notification UX is already planned *without new formats* (`TASK-0141`).

This task adds **multi-input bundling** and a **CLI surface** to package a reproducible report from:

- a single crash report (`.nxcd.zst`),
- a bounded log excerpt from `logd`,
- small system metadata files.

## Goal

Provide a deterministic, offline bundling pipeline:

- `nx diagnose ...` (as a subcommand of the canonical `nx` CLI) can:
  - list available crash reports (delegating to `nx crash`/`crashd`),
  - show a symbolized report (delegating to `nx crash show`/`crashd`),
  - create a deterministic bugreport bundle for a single crash id.
- The bundle format is deterministic and verifiable by host tests.
- OS/QEMU selftest can create a bundle and emit markers **only after** the artifact exists.

## Non-Goals

- Introducing new dump formats (`NXMD`, `.ncr`, etc.). We reuse `.nxcd(.zst)` from `TASK-0048/0049`.
- Introducing a new daemon name for bundling unless strictly required; prefer extending `crashd` export API (`TASK-0141`)
  and keeping `nx` as the orchestrator.
- Network upload, remote collectors, or online workflows.
- “Whole system snapshot” bundles (multiple reports, full `/state` export). v1 is **single crash id**.

## Constraints / invariants (hard requirements)

- **No new formats**: bundle contains existing artifacts; any new “manifest” must be an internal, versioned file *inside* the bundle.
- **No new URI scheme drift**: reuse existing `pkg:/`, `/state/...`, and `content://state/...` conventions (see `TASK-0141`).
- **Deterministic archive**:
  - stable entry ordering,
  - stable mtimes/uid/gid/mode normalization,
  - stable path normalization (no host absolute paths),
  - stable compression settings (if compressed).
- **Offline-only**: no network egress.
- **No fake success**: markers only after bundle write succeeds and size/hash checks pass.

## Red flags / decision points

- **RED (dependency gates)**:
  - Requires `/state` (`TASK-0009`) and a functioning crash pipeline (`TASK-0049`) or at minimum the v1 crash artifacts (`TASK-0018`).
  - Requires log query/export from `logd` (`TASK-0006`) in a bounded way.
- **YELLOW (CLI drift)**:
  - Do not create a separate `nx-diagnose` binary unless the `nx` CLI cannot host subcommands. Prefer `nx diagnose`.
- **YELLOW (hash stability)**:
  - Bundle hash stability is only meaningful if inputs are stable. Host tests must use fixtures; OS selftest may only assert
    “exists + bounded size + contains required files” unless inputs are fixed fixtures.

## Contract sources (single source of truth)

- QEMU marker contract: `scripts/qemu-test.sh`
- Crash artifacts contract: `TASK-0048`/`TASK-0049` (`.nxcd(.zst)` and `.report.txt`)
- Export/redaction contract: `TASK-0141`
- log query contract: `TASK-0006`

## Stop conditions (Definition of Done)

### Proof (Host) — required

- `cargo test -p diagnose_bundle_host` green (new)
  - Given fixture inputs (fixture `.nxcd`, fixture `logd.ndjson`, fixture metadata files), bundling produces:
    - stable file list,
    - stable archive hash across runs.
  - Bundle parser (if any) validates required entries and bundle manifest version.

### Proof (OS/QEMU) — gated

- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=120s ./scripts/qemu-test.sh`
- Markers:
  - `SELFTEST: diagnose bundle ok`

## Bundle v1 contents (minimum)

Bundle must contain these paths (all deterministic, small, bounded):

- `/crash/report.txt` (from crash pipeline)
- `/crash/dump.nxcd.zst` (the crash artifact)
- `/logs/logd.ndjson` (bounded excerpt: last N records or last T seconds; deterministic selection policy)
- `/system/os-release` (or equivalent canonical file)
- `/system/build.prop` (already exists in `pkg:/system/build.prop`; include the resolved content)
- `/system/update.json` (only if already defined by updates tasks; otherwise omit in v1)
- `/bundle/manifest.json` (inside-bundle manifest, versioned, stable keys; includes crash id, build-id, and included file list)

## Touched paths (allowlist)

- `tools/nx/` (add `nx diagnose` subcommand; host-friendly)
- `source/services/crashd/` (optional: add “export bundle inputs” RPC if needed; prefer reusing `TASK-0141` export surface)
- `source/services/logd/` (optional: bounded export helper if query isn’t sufficient)
- `source/apps/selftest-client/` (marker)
- `tests/diagnose_bundle_host/` (new)
- `docs/reliability/` and `docs/tools/`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. **Decide the bundle container**
   - Prefer `tar` (optionally `tar.gz`) with deterministic packing rules.
   - Document deterministic archive normalization rules and enforce in host tests.
2. **Implement host-first bundling library + tests**
   - Fixture-driven bundling and stable hash proof.
3. **Wire `nx diagnose` commands (host-first)**
   - `nx diagnose bundle <id> --out <path>` (delegates to crash tools + log fixtures for host tests).
4. **OS wiring (gated)**
   - Selftest triggers a controlled crash, then runs `nx diagnose bundle` using OS export surfaces and emits marker.
5. **Docs**
   - `docs/diagnostics/bugreports.md` (bundle contents, determinism rules, offline workflow).

## Acceptance criteria (behavioral)

- `nx diagnose bundle <id>` produces a deterministic bundle for deterministic inputs and refuses to claim success otherwise.
- Bundle contains the required files and a versioned manifest.
- OS/QEMU emits `SELFTEST: diagnose bundle ok` only after bundle exists (and passes minimal verification).
