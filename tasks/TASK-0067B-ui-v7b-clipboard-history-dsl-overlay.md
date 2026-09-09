---
title: TASK-0067B UI v7b follow-up: clipboard history panel in the desktop shell (DSL) + copy-back
status: Draft (end-state rewrite 2026-09-09; blocked on TASK-0067)
owner: @ui
created: 2026-03-28
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Clipboard v2 service baseline: tasks/TASK-0067-ui-v7b-dnd-clipboard-v2.md
  - SystemUI bootstrap shell: tasks/TASK-0080B-systemui-dsl-bootstrap-shell-launcher-host.md
  - DSL App Integration Kit: tasks/TASK-0122C-dsl-app-integration-kit-v1-picker-clipboard-share-print.md
---

## End-state rewrite 2026-09-09 (binding; supersedes older sections where they differ)

**Ground truth 2026-09-09:** nothing exists — no `.nx` component, page or store mentions the
clipboard; no shell entry point; no bridge (TASK-0122C is Draft); the named proof dir
`tests/ui_v7b_clipboard_history_host/` was never created. Hard dependency: TASK-0067.

### Goal (end system)

A desktop-shell panel (DSL) listing clipboardd history with MIME-aware previews and copy-back,
consuming `svc.clipboard.*` only.

### Non-goals

Any storage in the shell; sync; editing items; a second clipboard UI anywhere else.

### Invariants

Reducers pure; reads/writes are effects with `timeoutMs`; list bounded by clipboardd's
`HISTORY_MAX` (≤ 16, no paging); preview text ≤ 120 chars derived in the store, never in views.

### Decisions

- **D1 Surface** = `userspace/apps/desktop-shell/ui/components/panels/ClipboardPanel.nx`
  hosted by `PanelHost.nx` (same shape as `SoundPanel`/`WifiPanel`); store
  `ui/composables/clipboard.store.nx` (`svc.clipboard.list/restore/clear`; `OP_WATCH` events
  re-list through app-host's push intake like `WindowsChanged`).
- **D2 Entry point** = a topbar tile (`components/topbar`); dismissal via the TASK-0074
  overlay contract (`.overlay(modal)` + `onDismiss`).
- **D3 Rows** = `List` with `.key(seq)` (≤ 16 entries; no virtualization needed).
- **D4 Permission** — shell manifest `caps += "nexus.permission.CLIPBOARD"`; history reads are
  allowed because the shell is the desktop owner (0067 D3), no extra policy cap.
- **D5 Proof home** = `tests/dsl_apps_conformance` (existing); NO new proof crate.

### Packages

- **P1** store + panel + `TranscriptHost` fixtures (list / restore / empty / denied). Blast:
  `dsl_apps_conformance`, `ui_v10_goldens`.
- **P2** shell wiring + visible-lane pixel proof region. Blast: visible lane,
  `systemui_bootstrap_shell_host`.

### Definition of Done

Host: snapshot goldens light/dark (empty, 3 items, denied); interaction fixture "tap row →
`restore(seq)` dispatched → list re-ordered newest-first". QEMU: `apphost: dsl svc
clipboard.restore ok (seq=…)`, `SELFTEST: ui v7 clipboard history ok` (selftest writes 3 items,
`OP_LIST` newest-first, `OP_RESTORE` moves seq to head) + a visible-lane pixel-proof region for
the open panel (`tools/pixel_proof_judge.py`); markers registered in `ui.toml`,
`qemu-test.sh`, `markers.txt` (windowd contract only). Docs: `transfer-sharing/clipboard.md`
"History surface".

### Touched paths

`userspace/apps/desktop-shell/{manifest.toml,ui/components/panels/ClipboardPanel.nx,
ui/components/panels/PanelHost.nx,ui/composables/clipboard.store.nx,ui/components/topbar/*}`,
`tests/dsl_apps_conformance/`, markers triple.

### Dependencies

TASK-0067 (service + binding), TASK-0074 (dismissal contract).

## Context — historical, superseded by the end-state rewrite above

`TASK-0067` establishes the service and routing side of clipboard and DnD.
To make clipboard behavior actually testable and user-facing, we also need a visible clipboard history surface.

This follow-up keeps the service and the UI separate:

- `TASK-0067` owns `clipboardd`
- this task owns the visible DSL overlay/app

## Goal — historical, superseded by the end-state rewrite above

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
