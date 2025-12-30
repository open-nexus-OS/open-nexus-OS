---
title: TASK-0127 Share v2b: SystemUI chooser overlay + first-party targets (Clipboard/Save/Notes) + grants plumbing + markers
status: Draft
owner: @ui
created: 2025-12-25
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Share v2a intentsd/policy: tasks/TASK-0126-share-v2a-intentsd-registry-dispatch-policy-host.md
  - Grants (content://): tasks/TASK-0084-ui-v12a-scoped-uri-grants.md
  - contentd saveAs: tasks/TASK-0112-ui-v19b-contentd-saveas-downloads.md
  - Clipboard v3: tasks/TASK-0087-ui-v13a-clipboard-v3.md
  - MIME defaults (chooser “always use”): tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md
  - Notes open/save integration: tasks/TASK-0083-ui-v11c-document-picker-open-save-openwith.md
---

## Context

With `intentsd` (Share v2a) we can build a real share experience:

- a chooser overlay that selects a target app/provider,
- result callbacks to show toasts/success,
- first-party share targets for common workflows,
- strict enforcement of `content://` + grant tokens across subjects.

## Goal

Deliver:

1. SystemUI chooser overlay:
   - queries `intentsd.queryTargets(mime)`
   - displays icon+label grid, search filter, and an “Always use for this type” toggle
   - if “Always use” set, call `mimed.setDefault(mime, appId)`; otherwise update MRU via `intentsd.bumpRank`
   - dispatch selected target and await result; show toast derived from `Result.status`
   - markers:
     - `chooser: open (mime=..., n=...)`
     - `chooser: pick app=...`
     - `chooser: result status=...`
2. First-party share targets (receivers) + registration on startup:
   - Clipboard target:
     - accepts text/html/image/URIs and writes to `clipboardd` (v3 flavors preferred; fall back if not available)
     - marker: `share: clipboard ok (mimes=...)`
   - Save-to-Files target:
     - uses `contentd.saveAs` into `state://Downloads/` (v1 fixed destination is acceptable)
     - marker: `share: save ok (uri=...)`
   - Notes target:
     - registers for `text/plain` and optionally `image/png` / `text/html` (HTML→plain conversion)
     - creates a new note and returns `outUri=content://state/notes/<id>`
     - marker: `notes: share received (mime=...)`
3. Grants plumbing:
   - sender obtains grant token(s) from `grantsd` for `content://` URIs
   - if chooser selection differs from a pre-granted subject, use `regrant` for the chosen target subject
   - receivers must pass the grant token to `contentd.openUri` (enforced by `TASK-0084`)
   - TTL behavior documented; revoke optional

## Non-Goals

- Kernel changes.
- Wiring every sender app (follow-up task).
- Full “save destination picker” UX (can start with Downloads fixed path).

## Constraints / invariants (hard requirements)

- No fake success: chooser “result status” must be produced by a real receiver response.
- Deterministic ordering and deterministic chooser behavior for tests.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/share_v2_host/` (extended or split into `tests/share_v2_targets_host/`):

- chooser model receives deterministic target ordering (MRU + mimed default)
- dispatch to clipboard/save/notes targets yields deterministic `Result.status`
- grants required for `content://` cross-subject access; missing/invalid token denied deterministically

### Proof (OS/QEMU) — gated

UART markers include:

- `chooser: open (mime=..., n=...)`
- `share: clipboard ok`
- `share: save ok`
- `notes: share received (mime=...)`

## Touched paths (allowlist)

- SystemUI chooser overlay plugin(s)
- `userspace/apps/share/` (targets; new)
- `userspace/apps/notes/` (target registration and handler)
- `source/apps/selftest-client/` (wired in follow-up task)

## Plan (small PRs)

1. chooser overlay UI + markers
2. clipboard/save/notes targets + registration + markers
3. grants plumbing end-to-end + host tests

