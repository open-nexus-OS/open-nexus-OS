---
title: TASK-0067B UI v7b follow-up: Clipboard History DSL overlay/app + launcher/system share hooks
status: Draft
owner: @ui
created: 2026-03-28
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Clipboard v2 service baseline: tasks/TASK-0067-ui-v7b-dnd-clipboard-v2.md
  - SystemUI bootstrap shell: tasks/TASK-0080B-systemui-dsl-bootstrap-shell-launcher-host.md
  - DSL App Integration Kit: tasks/TASK-0122C-dsl-app-integration-kit-v1-picker-clipboard-share-print.md
---

## Context

`TASK-0067` establishes the service and routing side of clipboard and DnD.
To make clipboard behavior actually testable and user-facing, we also need a visible clipboard history surface.

This follow-up keeps the service and the UI separate:

- `TASK-0067` owns `clipboardd`
- this task owns the visible DSL overlay/app

## Goal

Deliver:

1. Clipboard History DSL UI:
   - overlay or small app surface rendered with the canonical DSL structure
   - list/history of recent clipboard items with MIME-aware preview
2. Integration:
   - launcher/system entry point
   - paste or copy-back action through clipboard bridge
3. Host tests + OS markers for visible clipboard behavior.

## Non-Goals

- Replacing `clipboardd`.
- Full cross-device clipboard sync.

## Constraints / invariants (hard requirements)

- UI consumes `clipboardd`; it does not reimplement clipboard storage.
- Reducers remain pure; read/write actions go through effects.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- snapshot and interaction tests for clipboard history UI

### Proof (OS/QEMU) — gated

- visible clipboard history surface opens and can restore a previous entry deterministically

## Touched paths (allowlist)

- SystemUI/launcher integration points
- clipboard DSL UI package(s)
- `tests/ui_v7b_clipboard_history_host/` (new)
- `docs/dev/ui/patterns/transfer-sharing/clipboard.md`

## Plan (small PRs)

1. DSL clipboard history UI
2. bridge integration + interactions
3. selftests + docs
