---
title: TASK-0098 UI v15e: rich text editor widget + Notes app (v1) + clipboard v3 paste + undo/redo + export (html/pdf)
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Zero-Copy App Platform (RichContent + paste mapping + audit/autosave): tasks/TRACK-ZEROCOPY-APP-PLATFORM.md
  - DSL v1 DevX track: tasks/TRACK-DSL-V1-DEVX.md
  - DSL app platform: tasks/TASK-0122B-dsl-app-platform-v1-shell-routing-launch-contract.md
  - DSL app integration kit: tasks/TASK-0122C-dsl-app-integration-kit-v1-picker-clipboard-share-print.md
  - DSL syntax/layout convention: tasks/TASK-0075-dsl-v0_1a-syntax-ir-cli.md
  - Office Suite (Word baseline): tasks/TRACK-OFFICE-SUITE.md
  - Text primitives: tasks/TASK-0094-ui-v15a-text-primitives-uax-bidi-hittest.md
  - Selection/TextField core: tasks/TASK-0095-ui-v15b-selection-caret-textfield-core.md
  - Clipboard v3: tasks/TASK-0087-ui-v13a-clipboard-v3.md
  - IME/OSK: tasks/TASK-0096-ui-v15c-ime-candidate-ui-osk.md
  - Spellcheck: tasks/TASK-0097-ui-v15d-spellcheck-spellerd.md
  - Print pipeline: tasks/TASK-0088-ui-v13b-print-to-pdf-printd-preview.md
  - Design kit baseline: tasks/TASK-0073-ui-v10a-design-system-primitives-goldens.md
  - Share v2 targets (Notes as a share target): tasks/TASK-0127-share-v2b-chooser-ui-targets-grants.md
---

## Context

After primitives + selection + IME + spellcheck are in place, we can build a rich text editor:

- attributed runs (bold/italic/underline/code/link),
- lists and paragraph styles,
- undo/redo,
- clipboard v3 paste mapping (html/rtf/image),
- and export to HTML/PDF.

This task ships the widget and a **Notes v1** app to validate the whole pipeline.

Brand stance:

- The user-facing app is **Notes** (not “RichText demo”).
- The `richtext` widget remains a reusable UI component.

## Goal

Deliver:

1. `ui/kit/richtext` widget:
   - this is a **blessed DSL primitive**, not an ad-hoc app-specific widget
   - it expands the DSL toward the `TRACK-DSL-V1-DEVX` "pro surfaces" path
   - attributed run model (inline + paragraph)
   - commands (toggle bold/italic/underline/code, lists, links)
   - undo/redo stack (bounded)
   - paste mapping:
     - html → sanitized subset → attributed runs
     - rtf → minimal mapper → runs
     - image/png → insert attachment placeholder (URI-based, stub allowed but explicit)
   - a11y semantics (caret/selection and format announcements)
2. `userspace/apps/notes`:
   - visible app shell/chrome is authored directly in the DSL:
     - `ui/pages/NotesPage.nx`
     - `ui/components/**.nx`
     - `ui/composables/**.store.nx` for pure note state/export/autosave indicators
     - `ui/services/**.service.nx` for picker/clipboard/share/print/autosave effect adapters
   - the Notes page may colocate `Store`, `Event`, `reduce`, `@effect`, and `Page` while the app is still small
   - the rich text editor surface is mounted through the blessed `ui/kit/richtext` primitive rather than bypassing DSL layout/state/effects
   - toolbar + status bar (words/chars/lang)
   - autosave to `state:/notes/.autosave/...` (reuse patterns)
   - export:
     - HTML export (sanitized)
     - PDF export via `printd.renderView`
   - markers:
     - `notes: open uri=...`
     - `notes: export html ok`
     - `notes: export pdf ok`
3. Host tests for HTML paste mapping, undo/redo, export hooks, and Notes DSL shell behavior (mocked printd).

## Non-Goals

- Kernel changes.
- Full DOCX/ODT import/export.

## Constraints / invariants

- Deterministic paste mapping for fixture HTML/RTF (goldens).
- Bounded memory:
  - cap undo depth,
  - cap document length,
  - cap attachment sizes.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v15e_host/`:

- HTML → attributed runs mapping matches goldens
- undo/redo produces deterministic document states
- export html stable for fixture doc
- export pdf triggers printd render call (mocked)
- Notes DSL shell snapshots/interactions are deterministic under host fixtures

### Proof (OS/QEMU) — gated

UART markers:

- `SELFTEST: ui v15 rte ok`

## Touched paths (allowlist)

- `userspace/ui/kit/richtext/` (new)
- `userspace/apps/notes/` (new)
- `userspace/apps/notes/ui/` (DSL pages/components/composables/services)
- `tests/ui_v15e_host/`
- `docs/dev/ui/richtext.md` (new)

## Plan (small PRs)

1. attributed model + basic rendering + selection integration
2. commands + undo/redo + clipboard paste mapping
3. app + autosave + export html/pdf
4. tests + docs
