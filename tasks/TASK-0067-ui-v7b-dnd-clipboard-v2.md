---
title: TASK-0067 UI v7b: drag-and-drop controller (typed offers) + clipboard v2 (MIME-aware, history, policy)
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - UI v2a input routing baseline: tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md
  - UI v6b app lifecycle baseline (focus): tasks/TASK-0065-ui-v6b-app-lifecycle-notifications-navigation.md
  - Policy as Code (clipboard guards): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Config broker (clipboard budgets): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

To make the system productive, we need interoperable content transfer:

- drag-and-drop with typed payload negotiation,
- a robust clipboard with MIME support and history.

Both must be policy-guarded (focus/foreground constraints) and bounded (budgets).

Screenshot/share is handled separately (v7c).

## Goal

Deliver:

1. DnD protocol + routing in `windowd`:
   - DragSource/DropTarget interfaces
   - global DnD controller: enter/over/leave/drop
   - drag image overlay (VMO-backed, bounded)
   - negotiated pull (`read(mime)` after accept)
2. Clipboard v2 service `clipboardd`:
   - multi-MIME items
   - history ring with configurable size and eviction
   - policy gating: focused/foreground subjects
3. SystemUI integration hooks (minimal):
   - clipboard history popup stub (optional for v7b)
4. Host tests + OS markers.

## Non-Goals

- Kernel changes.
- Full OS-wide file manager integration.
- Screenshot/share sheet (v7c).

## Constraints / invariants (hard requirements)

- Deterministic negotiation:
  - stable MIME preference order,
  - stable accept/reject reasons.
- Bounded memory:
  - cap drag image bytes,
  - cap clipboard item bytes,
  - cap history length.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v7b_host/`:

- DnD negotiation:
  - offer `{text/plain,image/png}` → target selects `text/plain` → drop ok
  - reject case produces deterministic trace/reason
- clipboard:
  - write multi-MIME item
  - read preferred mime returns expected data
  - history ring evicts oldest deterministically

### Proof (OS/QEMU) — gated

UART markers:

- `windowd: dnd on`
- `dnd: enter(target=..., mimes=...)`
- `dnd: drop ok (mime=...)`
- `clipboardd: ready`
- `clipboard: write ok (mimes=...)`
- `clipboard: read ok (mime=...)`
- `SELFTEST: ui v7 dnd ok`
- `SELFTEST: ui v7 clipboard ok`

## Touched paths (allowlist)

- `source/services/windowd/idl/dnd.capnp` + `source/services/windowd/` (DnD)
- `source/services/clipboardd/` (new)
- `tests/ui_v7b_host/`
- `source/apps/selftest-client/`
- `tools/postflight-ui-v7b.sh` (delegates)
- `docs/dev/ui/dnd.md` + `docs/dev/ui/clipboard.md`

## Plan (small PRs)

1. dnd IDL + controller + drag image overlay + markers
2. clipboardd + history ring + policy/budgets + markers
3. host tests + OS markers + docs + postflight
