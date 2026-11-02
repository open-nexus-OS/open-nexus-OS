---
schema: 1
id: TRACK-SCORE-APP
title: "Score App (forScore-class): fast PDF sheet music reader + annotations + setlists + page turn, offline-first and deterministic"
status: draft
owner: ["@apps", "@ui"]
created: 2026-01-19
links:
  - label: Vision
    target: docs/architecture/vision.md
  - label: Playbook
    target: CLAUDE.md
  - label: "Authority & naming registry"
    target: tasks/TRACK-AUTHORITY-NAMING.md
  - label: "Content/URIs + picker + grants"
    target: "tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md, tasks/TASK-0083-ui-v11c-document-picker-open-save-openwith.md, tasks/TASK-0084-ui-v12a-scoped-uri-grants.md"
  - label: "Files app integration (open-with/share)"
    target: tasks/TASK-0086-ui-v12c-files-app-progress-dnd-share-openwith.md
  - label: "PDF viewer baseline"
    target: tasks/TASK-0092-ui-v14b-pdf-viewer.md
  - label: "Print/export helpers (optional)"
    target: tasks/TASK-0088-ui-v13b-print-to-pdf-printd-preview.md
  - label: "Input spine (page turn via HID)"
    target: tasks/TASK-0253-input-v1_0b-os-hidrawd-touchd-inputd-ime-hooks-selftests.md
  - label: "windowd compositor/present spine"
    target: tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md
---

## Goal (track-level)

Deliver a first-party **Score** app comparable to forScore (interaction patterns only; no trade dress):

- fast PDF sheet music viewer (page turn, zoom, crop margins),
- **setlists** (performance mode) and quick switching,
- **annotations** (pen/highlighter/text notes) stored non-destructively,
- offline-first library with bounded indexing/search,
- page-turn via touch + optional keyboard/HID pedals,
- deterministic proofs (render correctness, annotation replay, bounds).

## Non-goals (avoid drift)

- Not a general-purpose PDF editor.
- No unbounded OCR/ML in v1.
- No cloud sync by default (can be added later and must be policy-gated).

## Authority model (must match registry)

Score is an app. It consumes:

- `windowd` (present),
- `contentd`/`mimed`/`grantsd` for open/save/import/export (pathless),
- `inputd` for page-turn control (no direct HID node access),
- `policyd` for any sensitive operations (later).

## Core features (v1 scope)

- **Library**:
  - import PDFs into app library (or reference by content:// with persistable grants if enabled)
  - tags + composer/title metadata (bounded)
- **Viewer**:
  - continuous or page mode
  - crop margins and remember per-score (bounded)
  - bookmarks and “repeats” markers (optional)
- **Setlists**:
  - ordered setlists, per-item notes
  - performance mode (big buttons, minimal UI)
- **Annotations**:
  - pen/highlighter, erase, undo/redo (bounded)
  - non-destructive overlay model + deterministic replay
- **Page turn**:
  - tap/gesture
  - optional HID pedal mapping via `inputd`

## Gates / blockers

- PDF render baseline: `tasks/TASK-0092-ui-v14b-pdf-viewer.md`
- Content/picker/grants: `TASK-0081`/`0083`/`0084`
- Input spine: `TASK-0253` (for pedals/hotkeys)
- Present spine: `TASK-0055`

## Phase map

### Phase 0 — Host-first Score core

- PDF viewer integration (bounded fixtures) + fast page navigation
- annotation overlay model + deterministic replay tests
- setlist model tests

### Phase 1 — OS wiring

- open/import via picker + grants
- HID pedal page-turn via `inputd`
- QEMU markers only after real open + page turn + annotate operations

### Phase 2 — Pro polish

- better library search (bounded)
- export annotated PDF (optional; bounded) via print pipeline where applicable

## Candidate subtasks (to be extracted into TASK-XXXX)

- **CAND-SCORE-000: Library + setlists v0 (bounded metadata) + host tests**
- **CAND-SCORE-010: PDF viewer wiring v0 (fast page turn, crop memory) + fixtures**
- **CAND-SCORE-020: Annotation overlay v0 (strokes + replay) + deterministic tests**
- **CAND-SCORE-030: Page-turn controls v0 (touch + inputd pedal) + markers**

## Extraction rules

Candidates become real tasks only when they:

- define bounds (max PDF bytes/pages, max strokes, max setlist items),
- prove determinism (fixture PDFs, stroke replay, stable crop behavior),
- keep authority boundaries (no raw paths; no direct device nodes).
