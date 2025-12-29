---
title: TASK-0093 UI v14c: Markdown viewer + find-in-page + export-to-PDF + nx md export + Open With wiring
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Document access (picker/content): tasks/TASK-0083-ui-v11c-document-picker-open-save-openwith.md
  - MIME/content foundations: tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md
  - Print pipeline: tasks/TASK-0088-ui-v13b-print-to-pdf-printd-preview.md
  - Searchd (optional discoverability): tasks/TASK-0071-ui-v9a-searchd-command-palette.md
---

## Context

We need a lightweight Markdown consumer app with PDF export, built on:

- content URIs and picker,
- our UI runtime/layout/kit,
- the print-to-PDF pipeline (`printd`).

This task delivers Markdown viewer + export + headless CLI helper and Open With registration.
PDF viewer is in `TASK-0092`.

## Goal

Deliver:

1. `userspace/apps/markdown`:
   - open `text/markdown` via picker or Open With (`openUri`)
   - parse Markdown subset into a render tree (headings/lists/links/images/code)
   - find-in-page (simple substring match) with highlights
   - export current view to PDF via `printd.renderView` and Save As (picker)
   - markers:
     - `markdown: open uri=...`
     - `markdown: export pdf ok`
2. Open With integration:
   - register handlers for `text/markdown` and `text/x-markdown`
   - launcher tile for Markdown
3. CLI helper `nx md export` (host-first):
   - headless render markdown from a URI (or host path for tests) and export PDF
   - marker: `nx: md export ok (bytes=...)`
4. Host tests for markdown rendering snapshots and export determinism.

## Non-Goals

- Kernel changes.
- Full CommonMark spec.
- External HTTP links in v14 (disabled by policy; only content://).

## Constraints / invariants (hard requirements)

- Deterministic rendering for fixture markdown (goldens).
- Bounded parsing and rendering:
  - cap document size and nesting depth.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v14c_host/`:

- render fixed markdown document (light/dark) snapshots match goldens (SSIM threshold if needed)
- `nx md export` produces a deterministic PDF (within documented metadata tolerance)

### Proof (OS/QEMU) — gated

UART markers:

- `markdown: open uri=...`
- `markdown: export pdf ok`
- `SELFTEST: ui v14 md ok`

## Touched paths (allowlist)

- `userspace/apps/markdown/` (new)
- `tools/nx-md/` (new)
- `services/mimed` registration + launcher tiles
- `tests/ui_v14c_host/`
- `docs/apps/markdown.md` (new)

## Plan (small PRs)

1. markdown parser subset + render tree + markers
2. export to PDF via printd + markers
3. nx md export + host tests + Open With wiring + docs
