---
title: TASK-0087 UI v13a: Clipboard v3 (html/rtf/image) + negotiation + budgets + markers
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Zero-Copy App Platform (clipboard canonical interchange): tasks/TRACK-ZEROCOPY-APP-PLATFORM.md
  - Office Suite (copy/paste across Word/Sheets/Slides): tasks/TRACK-OFFICE-SUITE.md
  - Clipboard v2 baseline: tasks/TASK-0067-ui-v7b-dnd-clipboard-v2.md
  - Policy as Code (clipboard guards): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Config broker (budgets): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
---

## Context

Clipboard v2 introduced multi-MIME items and history. UI v13 upgrades clipboard to v3:

- richer flavors (`text/html`, `text/rtf`, `image/png`),
- deterministic negotiation (preferMime),
- safe fallback conversions (`html`/`rtf` → plain),
- strict budgets with redaction markers.

Printing and apps that consume clipboard are separate tasks.

## Goal

Deliver:

1. `clipboardd` supports clipboard v3 flavors:
   - `text/plain`
   - `text/html` (sanitized fragment; scripts/styles stripped)
   - `text/rtf` (stored and converted to plain on fallback)
   - `image/png`
2. Read negotiation:
   - `preferMime` chooses best available match deterministically
   - fallback `text/html` → `text/plain` by stripping tags
3. Budgets:
   - per-flavor max bytes; oversize images redacted (store metadata, omit payload)
4. Markers:
   - `clipboard: write ok (mimes=...)`
   - `clipboard: read ok (mime=...)`
   - `clipboard: redact (mime=...)`
5. Host tests proving behavior deterministically.

## Non-Goals

- Kernel changes.
- Full HTML rendering engine; v3 only stores/sanitizes and provides plain fallback conversion.

## Constraints / invariants (hard requirements)

- Deterministic sanitization and conversion results for fixture inputs.
- Bounded memory: enforce max bytes per flavor and max item bytes.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v13a_host/`:

- write multi-flavor item and read back preferred mime deterministically
- read fallback conversion `text/html` → `text/plain`
- oversize image triggers `clipboard: redact (mime=image/png)` and read returns redacted payload behavior deterministically

## Touched paths (allowlist)

- `source/services/clipboardd/` (extend)
- `tests/ui_v13a_host/`
- `docs/platform/clipboard-v3.md` (new)

## Plan (small PRs)

1. add v3 flavors + negotiation rules + markers
2. add sanitization + budgets + redaction markers
3. host tests + docs
