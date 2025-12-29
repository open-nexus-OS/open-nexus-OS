---
title: TASK-0088 UI v13b: Print-to-PDF pipeline (pdfgen + printd) + preview UI (Print to PDF only) + markers
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Shaping/SVG baseline: tasks/TASK-0057-ui-v2b-text-shaping-svg-pipeline.md
  - Doc picker/save destination: tasks/TASK-0083-ui-v11c-document-picker-open-save-openwith.md
  - Persistence (/state spool): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Policy as Code (print guards): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
---

## Context

We want printing without real printers: **Print to PDF** only.
This requires:

- a minimal PDF writer,
- a `printd` service that can render from a view or from a document URI,
- a preview path producing PNGs for a print dialog UI.

Apps (text editor/image viewer) integrate in later tasks.

## Goal

Deliver:

1. `userspace/print/pdfgen`:
   - PDF 1.4 subset writer (pages, content streams, basic fonts, images)
   - deterministic output for fixed input scenes
2. `printd` service:
   - `renderView(viewId,spec) -> pdf VMO`
   - `renderDoc(uri,spec) -> pdf VMO`
   - `preview(uriOrView,page,dpi) -> png VMO`
   - spools PDF under `state:/prints/<ts>-<app>.pdf` and returns VMO/bytes
   - markers:
     - `printd: ready`
     - `print: job (pages=.. bytes=..)`
3. SystemUI print preview/dialog overlay (minimal):
   - fixed printer “Print to PDF”
   - calls preview and render; then uses doc picker Save As destination (gated)
   - markers:
     - `printui: open`
     - `printui: preview ok`
     - `printui: print ok (bytes=...)`
4. Host tests for PDF structure and preview determinism.

## Non-Goals

- Kernel changes.
- Real printer backends.
- Full PDF spec.

## Constraints / invariants (hard requirements)

- Deterministic PDF bytes for deterministic inputs (within the designed subset).
- Bounded memory for rendering and preview output.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v13b_host/`:

- pdfgen: write shaped text + simple SVG to a page; validate basic PDF structure (xref/trailer) deterministically
- preview: known scene → PNG checksum (or golden) stable

## Touched paths (allowlist)

- `userspace/print/pdfgen/` (new)
- `source/services/printd/` (new)
- SystemUI print preview overlay (new)
- `tests/ui_v13b_host/`
- `docs/ui/print.md` (new)

## Plan (small PRs)

1. pdfgen minimal writer + deterministic tests
2. printd IDL + render/preview + markers
3. SystemUI print preview overlay + markers
4. host tests + docs
