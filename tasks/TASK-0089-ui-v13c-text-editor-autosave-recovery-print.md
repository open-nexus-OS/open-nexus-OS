---
title: TASK-0089 UI v13c: Text Editor app (tabs + find/replace + autosave/recovery + syntax stubs) + print integration
status: Draft
owner: @ui
created: 2025-12-23
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - DSL app platform: tasks/TASK-0122B-dsl-app-platform-v1-shell-routing-launch-contract.md
  - DSL app integration kit: tasks/TASK-0122C-dsl-app-integration-kit-v1-picker-clipboard-share-print.md
  - DSL syntax/layout convention: tasks/TASK-0075-dsl-v0_1a-syntax-ir-cli.md
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
   - visible UI is authored directly in the DSL:
     - `ui/pages/TextEditorPage.nx`
     - `ui/components/**.nx`
     - `ui/composables/**.store.nx` for pure editor state/find/replace/tab logic
     - `ui/services/**.service.nx` for picker/clipboard/print/autosave effect adapters
   - `TextEditorPage.nx` may colocate `Store`, `Event`, `reduce`, `@effect`, and `Page` while the feature set is still small
   - open/save via picker (`content://` URIs, streams)
   - tabs
   - find/replace (plain; regex optional)
   - soft-wrap toggle
   - line numbers
   - syntax highlight stubs for `.nx/.rs/.toml` (fast tokenization; no full parser)
2. Autosave + recovery:
   - autosave every N seconds to `state:/<appId>/.autosave/...`
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
5. Host tests for autosave/recovery, print invocation, and DSL UI behavior (mocked printd).

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
- Text Editor DSL UI snapshots/interactions are deterministic under host fixtures

### Proof (OS/QEMU) — gated

UART markers:

- `text: autosave ok`
- `text: restore ok`
- `text: print ok`
- `SELFTEST: ui v13 text/print ok` (full flow lives in v13d or a v13 umbrella postflight)

## Touched paths (allowlist)

- `userspace/apps/text/` (new)
- `userspace/apps/text/ui/` (DSL pages/components/composables/services)
- `tests/ui_v13c_host/`
- `docs/dev/ui/components/media-and-content/text-editor.md` (new)

## Plan (small PRs)

1. text editor skeleton + open/save + markers
2. autosave/recovery + host tests
3. clipboard v3 paste handling + print integration
4. docs
