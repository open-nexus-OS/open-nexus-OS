---
title: TASK-0091 UI v14a: printd preview text-map option + find/highlight helper (find-in-page substrate)
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Print pipeline baseline: tasks/TASK-0088-ui-v13b-print-to-pdf-printd-preview.md
  - Shaping baseline: tasks/TASK-0057-ui-v2b-text-shaping-svg-pipeline.md
---

## Context

PDF and Markdown viewers need “find in page/document”. We don’t have a full PDF text extraction pipeline.
A pragmatic v14 approach is to enhance `printd.preview` to optionally return a compact **text map**
that maps rendered text spans to bounding boxes in page coordinates.

This task delivers the substrate: `printd` text-map output + a helper library to search spans and compute highlights.
The PDF viewer and Markdown viewer tasks consume this.

## Goal

Deliver:

1. `printd.preview` enhancement:
   - optional `withTextMap: Bool` flag
   - returns:
     - preview PNG VMO (existing behavior)
     - plus a compact JSON blob for `{spans:[{text,bbox:[x,y,w,h]}]}` in page coordinates
   - marker: `printd: textmap on`
2. `userspace/ui/preview_textmap` helper:
   - parse text-map JSON
   - search spans for a query and return highlight rectangles (page coords)
   - deterministic matching rules (case sensitivity default documented)
3. Host tests for determinism and basic correctness.

## Non-Goals

- Kernel changes.
- Full PDF text extraction.
- Complex search semantics (regex, diacritics folding) beyond deterministic substring match v1.

## Constraints / invariants (hard requirements)

- Deterministic span order and stable bbox values for a fixed input scene (within rounding rules).
- Bounded output:
  - cap span count per page,
  - cap JSON size.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v14a_host/`:

- known preview scene returns a text-map with expected span count
- searching term yields deterministic highlight rects (golden JSON)
- text-map JSON is stable across runs (byte-stable or normalized)

## Touched paths (allowlist)

- `source/services/printd/` (extend preview)
- `userspace/ui/preview_textmap/` (new)
- `tests/ui_v14a_host/`
- `docs/ui/print-preview.md` (extend with text-map)

## Plan (small PRs)

1. printd: add withTextMap option + marker
2. helper crate: parse/search/highlight + tests
3. docs update

