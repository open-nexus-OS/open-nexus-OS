---
title: TASK-0090 UI v13d: Image Viewer app (png/jpeg/svg) + zoom/pan/rotate/export + clipboard image + print integration
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Document access (picker/content): tasks/TASK-0083-ui-v11c-document-picker-open-save-openwith.md
  - MIME registry (Open With): tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md
  - Clipboard v3: tasks/TASK-0087-ui-v13a-clipboard-v3.md
  - Print pipeline: tasks/TASK-0088-ui-v13b-print-to-pdf-printd-preview.md
  - Recents: tasks/TASK-0082-ui-v11b-thumbnailer-recents.md
  - SVG mini pipeline: tasks/TASK-0057-ui-v2b-text-shaping-svg-pipeline.md
---

## Context

We need a basic image consumer tool that integrates with:

- Open With / content URIs,
- clipboard image flavor,
- print-to-PDF.

## Goal

Deliver:

1. `userspace/apps/images`:
   - open via picker or Open With arg (`openUri`)
   - decode PNG/JPEG and render SVG via existing safe subset pipeline
   - zoom (fit/actual), pan, rotate 90° steps, flip (optional)
   - export as PNG to a Save As destination (picker)
   - markers:
     - `images: open uri=...`
     - `images: export ok`
2. Clipboard:
   - copy current image to clipboard as `image/png`
3. Print integration:
   - “Print…” opens print preview overlay and calls `printd.renderView` for the viewer view
   - marker: `images: print ok`
4. Host tests for rotate/export and clipboard write (deterministic fixtures).

## Non-Goals

- Kernel changes.
- Full photo editor.

## Constraints / invariants (hard requirements)

- Deterministic export for fixture inputs (checksum/golden).
- Bounded decode (cap max pixels and input bytes).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v13d_host/`:

- open fixture image; rotate; export; exported PNG checksum matches golden
- copy to clipboard writes `image/png` and respects budgets

### Proof (OS/QEMU) — gated

UART markers:

- `images: open uri=...`
- `images: export ok`
- `images: print ok`
- `SELFTEST: ui v13 image/print ok` (full flow in a combined selftest)

## Touched paths (allowlist)

- `userspace/apps/images/` (new)
- `tests/ui_v13d_host/`
- `docs/ui/image-viewer.md` (new)

## Plan (small PRs)

1. viewer skeleton + open + zoom/pan + markers
2. rotate/export + host tests
3. clipboard copy + print integration
4. docs
