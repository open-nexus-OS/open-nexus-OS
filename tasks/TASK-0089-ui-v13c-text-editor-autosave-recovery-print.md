---
title: TASK-0089 UI v13c: Text Editor app (tabs + find/replace + autosave/recovery + syntax stubs) + print integration
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Document access (picker/content): tasks/TASK-0083-ui-v11c-document-picker-open-save-openwith.md
  - Grants (cross-app open): tasks/TASK-0084-ui-v12a-scoped-uri-grants.md
  - Clipboard v3: tasks/TASK-0087-ui-v13a-clipboard-v3.md
  - Print pipeline: tasks/TASK-0088-ui-v13b-print-to-pdf-printd-preview.md
  - Recents: tasks/TASK-0082-ui-v11b-thumbnailer-recents.md
---

## Context

We want a core text tool with safe persistence and a print path.
This task focuses on the Text Editor app only, reusing:

- document picker + content URIs,
- clipboard v3,
- print-to-PDF pipeline.

## Goal

Deliver:

1. `userspace/apps/text`:
   - open/save via picker (`content://` URIs, streams)
   - tabs
   - find/replace (plain; regex optional)
   - soft-wrap toggle
   - line numbers
   - syntax highlight stubs for `.nx/.rs/.toml` (fast tokenization; no full parser)
2. Autosave + recovery:
   - autosave every N seconds to `state://<appId>/.autosave/...`
   - on launch, detect newer autosaves and offer restore
   - markers:
     - `text: open uri=...`
     - `text: autosave ok`
     - `text: restore ok`
3. Clipboard v3 integration:
   - accept html/rtf/plain; store plain view
   - image/png paste path can be a stub (“save as file”) with explicit marker if not implemented
4. Print integration:
   - “Print…” opens print preview overlay and calls `printd.renderView` for the editor view
   - marker: `text: print ok`
5. Host tests for autosave/recovery and print invocation (mocked printd).

## Non-Goals

- Kernel changes.
- Full rich text editor.

## Constraints / invariants (hard requirements)

- Deterministic autosave intervals in tests (inject clock).
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Bounded file sizes (cap max open size and autosave size).

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v13c_host/`:

- autosave file written and newer than base
- simulated crash then restore picks autosave and emits marker
- print invocation produces expected call to printd (mocked)

### Proof (OS/QEMU) — gated

UART markers:

- `text: autosave ok`
- `text: restore ok`
- `text: print ok`
- `SELFTEST: ui v13 text/print ok` (full flow lives in v13d or a v13 umbrella postflight)

## Touched paths (allowlist)

- `userspace/apps/text/` (new)
- `tests/ui_v13c_host/`
- `docs/ui/text-editor.md` (new)

## Plan (small PRs)

1. text editor skeleton + open/save + markers
2. autosave/recovery + host tests
3. clipboard v3 paste handling + print integration
4. docs
