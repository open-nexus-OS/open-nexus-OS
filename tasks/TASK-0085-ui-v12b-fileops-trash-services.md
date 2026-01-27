---
title: TASK-0085 UI v12b: file operations manager (fileopsd) + per-app Trash/Restore (trashd) + progress streams
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Content providers: tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md
  - Scoped grants: tasks/TASK-0084-ui-v12a-scoped-uri-grants.md
  - Persistence (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Policy as Code: tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Removable storage track (copy/move to USB/SD via streams): tasks/TRACK-REMOVABLE-STORAGE.md
---

## Context

To build a usable Files experience we need:

- a background-capable operations service to copy/move/rename/mkdir and stream progress,
- Trash semantics (soft delete) with restore and retention.

This task provides the service backbone. The Files app UI is v12c.

## Goal

Deliver:

1. `fileopsd` service:
   - copy/move across providers via `contentd.openUri` read streams and `contentd.create` + write streams
   - rename/mkdir helpers
   - trash/restore operations delegated to `trashd` (or implemented internally, but must stay consistent)
   - cancellable ops and progress stream (queued/running/done/failed/canceled)
   - respects scoped grants when crossing app sandboxes
   - markers:
     - `fileopsd: ready`
     - `fileops: op (id=..., kind=..., n=...)`
     - `fileops: done (id=..., bytes=...)`
2. `trashd` service:
   - per-app trash under `state:/<appId>/.trash`
   - sidecar metadata and deterministic restore rules:
     - restore to original parent if possible, else `Recovered/`
   - purge/empty with retention policy
   - markers:
     - `trashd: ready`
     - `trash: moved`
     - `trash: restored`
     - `trash: purged`
3. Host tests proving progress monotonicity and correctness.

## Non-Goals

- Kernel changes.
- Full delta copy optimization; v12b is correctness-first with bounded chunking.
- Files app UI (v12c).
- Introducing “path copy” APIs to apps. Cross-provider moves/copies (including removable media) must remain stream-based through `fileopsd` and scoped grants (see `tasks/TRACK-REMOVABLE-STORAGE.md`).

## Constraints / invariants (hard requirements)

- Deterministic progress reporting (monotonic bytes/pct; stable terminal states).
- Bounded resource usage:
  - configurable concurrency,
  - chunked copy with max chunk size.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v12b_host/`:

- copy from `pkg://` to `state:/` shows monotonic progress and done=OK
- copy from `pkg://` to `state:/` shows monotonic progress and done=OK
- move + rename reflected in provider listings
- trash then restore returns to original or `Recovered/` deterministically
- cancel stops operation and produces canceled state

## Touched paths (allowlist)

- `source/services/fileopsd/` (new)
- `source/services/trashd/` (new)
- `tests/ui_v12b_host/`
- `docs/platform/fileops.md` + `docs/platform/trash.md` (new)

## Plan (small PRs)

1. fileopsd core (copy/move/rename/mkdir) + progress + markers
2. trashd core + sidecar format + markers
3. integrate fileopsd trash/restore + host tests + docs
