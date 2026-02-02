---
title: TRACK Notes App (first-party): rich notes with fast capture, share targets, offline-first storage (Notes v1 is TASK-0098)
status: Draft
owner: @apps @ui
created: 2026-01-19
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - System Delegation / System Surfaces (share targets + defaults): tasks/TRACK-SYSTEM-DELEGATION.md
  - Authority & naming registry: tasks/TRACK-AUTHORITY-NAMING.md
  - Zero-Copy App Platform (RichContent + autosave patterns): tasks/TRACK-ZEROCOPY-APP-PLATFORM.md
  - Richtext widget + Notes v1 task: tasks/TASK-0098-ui-v15e-richtext-widget-app.md
  - Content/URIs + picker + grants: tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md, tasks/TASK-0083-ui-v11c-document-picker-open-save-openwith.md, tasks/TASK-0084-ui-v12a-scoped-uri-grants.md
  - Share v2 intents + chooser targets: tasks/TASK-0126-share-v2a-intentsd-registry-dispatch-policy-host.md, tasks/TASK-0127-share-v2b-chooser-ui-targets-grants.md
  - Search foundations (optional later): tasks/TASK-0151-search-v2-ui-host-command-palette-model-a11y.md
---

## Goal (track-level)

Deliver a first-party **Notes** app that is:

- fast for capture (open → type immediately),
- offline-first (local storage under `/state`),
- rich enough for everyday use (rich text, links, lists, attachments phased),
- a system integration anchor (share target, open-with, export).

## Scope boundaries (anti-drift)

- Notes is not “Word” and not “Docs”.
- Collaboration/sync is optional later and must be bounded and policy-gated.
- Avoid format lock-in: internal model is stable; exports are derived.

## Authority model (must match registry)

Notes is an app. It consumes:

- `contentd`/`mimed`/`grantsd` for open/save/share via `content://` (no raw paths),
- `policyd` for permission decisions (if network/sync is ever added),
- `logd` for audit/log sink (no secrets).

Notes must not implement its own parallel content broker or policy engine.

## System Delegation integration

Notes should be a first-class **system surface** for capture:
- register as a Share v2 target (text/html/image within budgets),
- support “quick capture” entry points that other apps can delegate to (without embedding note editors).

## Phase map

### Phase 0 — Notes v1 (RichText + autosave + export)

Implemented by the core task:

- `tasks/TASK-0098-ui-v15e-richtext-widget-app.md`

### Phase 1 — Notes as share target + library UX

- register as a Share v2 target (`TASK-0127`)
- basic notes library (list, pinned, tags bounded)
- import/export via picker/grants (no paths)

### Phase 2 — Attachments + quick capture polish

- image attachments (bounded) with safe storage
- quick capture surfaces (optional): launcher quick action / command palette

## Candidate subtasks (to be extracted into real TASK-XXXX)

- **CAND-NOTES-000: Notes library v0 (list/pin/tags bounded) + deterministic tests**
- **CAND-NOTES-010: Share target v0 (text/html/image → note) + grants enforcement**
- **CAND-NOTES-020: Attachments v0 (bounded images/files) + export semantics**

## Extraction rules

Candidates become real tasks only when they:

- define explicit bounds (note size, attachment sizes, list length),
- keep deterministic host proofs (goldens for paste mapping/export),
- keep authority boundaries (content/grants/policy centralized).
