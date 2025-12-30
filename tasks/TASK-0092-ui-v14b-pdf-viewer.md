---
title: TASK-0092 UI v14b: PDF Viewer app (preview raster pages) + zoom/pan/nav + find-in-doc + share/export + Open With
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Document access (picker/open-with): tasks/TASK-0083-ui-v11c-document-picker-open-save-openwith.md
  - MIME/content foundations: tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md
  - Thumbnailer (page thumbs cache): tasks/TASK-0082-ui-v11b-thumbnailer-recents.md
  - Print preview text-map substrate: tasks/TASK-0091-ui-v14a-printd-textmap-find-helper.md
  - Share sheet (optional): tasks/TASK-0068-ui-v7c-screenshot-screencap-share-sheet.md
---

## Context

We need a PDF consumer app. Without a full PDF renderer stack, we can reuse `printd.preview` to rasterize pages
and provide a text-map for find/highlights.

This task delivers the PDF Viewer app and Open With wiring for `application/pdf`.
Markdown viewer/export is in `TASK-0093`.

## Goal

Deliver:

1. `userspace/apps/pdf`:
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
3. Host tests for find behavior and preview integration (fixture PDF).

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

### Proof (OS/QEMU) — gated

UART markers:

- `pdf: open uri=... pages=...`
- `pdf: find hit n=...`
- `SELFTEST: ui v14 pdf ok`

## Touched paths (allowlist)

- `userspace/apps/pdf/` (new)
- `services/mimed` registration + launcher tiles (if not already wired)
- `tests/ui_v14b_host/`
- `docs/apps/pdf-viewer.md` (new)

## Plan (small PRs)

1. pdf app skeleton + open + page nav + markers
2. find/highlight via text-map helper + host tests
3. export/share + Open With wiring + docs

