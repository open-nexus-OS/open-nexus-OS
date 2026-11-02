---
schema: 1
id: TASK-0092
title: "UI v14b: PDF Viewer app (preview raster pages) + zoom/pan/nav + find-in-doc + share/export + Open With"
status: draft
owner: ["@ui"]
created: 2025-12-23
subsystem: ui
tracks: [TRACK-DSL-V1-DEVX]
links:
  - label: Vision
    target: docs/architecture/vision.md
  - label: Playbook
    target: CLAUDE.md
  - label: "DSL app platform"
    target: tasks/TASK-0122B-dsl-app-platform-v1-shell-routing-launch-contract.md
  - label: "DSL app integration kit"
    target: tasks/TASK-0122C-dsl-app-integration-kit-v1-picker-clipboard-share-print.md
  - label: "DSL syntax/layout convention"
    target: tasks/TASK-0075-dsl-v0_1a-syntax-ir-cli.md
  - label: "Document access (picker/open-with)"
    target: tasks/TASK-0083-ui-v11c-document-picker-open-save-openwith.md
  - label: "MIME/content foundations"
    target: tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md
  - label: "Thumbnailer (page thumbs cache)"
    target: tasks/TASK-0082-ui-v11b-thumbnailer-recents.md
  - label: "Print preview text-map substrate"
    target: tasks/TASK-0091-ui-v14a-printd-textmap-find-helper.md
  - label: "Share sheet (optional)"
    target: tasks/TASK-0068-ui-v7c-screenshot-screencap-share-sheet.md
---

## Context

We need a PDF consumer app. Without a full PDF renderer stack, we can reuse `printd.preview` to rasterize pages
and provide a text-map for find/highlights.

This task delivers the PDF Viewer app and Open With wiring for `application/pdf`.
Markdown viewer/export is in `TASK-0093`.

## Goal

Deliver:

1. `userspace/apps/pdf`:
   - visible app chrome is authored directly in the DSL:
     - `ui/pages/PdfViewerPage.nx`
     - `ui/components/**.nx`
     - `ui/composables/**.store.nx` for pure viewer/find/navigation state
     - `ui/services/**.service.nx` for picker/open-with/share/print effect adapters
   - the page may colocate `Store`, `Event`, `reduce`, `@effect`, and `Page` while the app remains small
   - the PDF page viewport itself may use a **blessed** document canvas / `NativeWidget` path for:
     - large page raster viewport
     - pan/zoom surface
     - highlight overlay composition
   - this task must feed requirements back into `TRACK-DSL-V1-DEVX` rather than inventing a one-off canvas API
   - open `application/pdf` via picker or Open With arg (`openUri`)
   - page thumbnails strip and main page canvas
   - zoom (fit/actual/step), pan, page navigation (prev/next/jump), rotate 90° steps
   - find-in-document using `printd.preview(withTextMap=true)` + `preview_textmap` helper:
     - incremental search
     - highlight rect overlays
   - share:
     - export current page as PNG (from preview)
     - share whole doc as PDF (pass-through URI)
   - markers:
     - `pdf: open uri=... pages=...`
     - `pdf: find hit n=...`
     - `pdf: export page ok`
2. MIME/Open With integration:
   - register `apps/pdf` for `application/pdf`
   - launcher tile for PDF Viewer
3. Host tests for find behavior, preview integration, and DSL UI behavior (fixture PDF).

## Non-Goals

- Kernel changes.
- True vector PDF rendering.
- Annotations/forms.

## Constraints / invariants (hard requirements)

- Use `content://` URIs only (no raw filesystem).
- Bounded preview caching (cap cached pages/thumbs).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v14b_host/`:

- open fixture PDF (or a stubbed printd that returns deterministic pages)
- find term produces deterministic hit count and highlight rects
- export page PNG checksum matches golden
- PDF Viewer DSL chrome snapshots/interactions are deterministic under host fixtures

### Proof (OS/QEMU) — gated

UART markers:

- `pdf: open uri=... pages=...`
- `pdf: find hit n=...`
- `SELFTEST: ui v14 pdf ok`

## Touched paths (allowlist)

- `userspace/apps/pdf/` (new)
- `userspace/apps/pdf/ui/` (DSL pages/components/composables/services)
- `services/mimed` registration + launcher tiles (if not already wired)
- `tests/ui_v14b_host/`
- `docs/apps/pdf-viewer.md` (new)

## Plan (small PRs)

1. pdf app skeleton + open + page nav + markers
2. find/highlight via text-map helper + host tests
3. export/share + Open With wiring + docs
