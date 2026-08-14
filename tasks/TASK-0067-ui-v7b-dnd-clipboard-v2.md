---
title: TASK-0067 UI v7b: drag-and-drop controller (typed offers) + clipboard v2 (MIME-aware, history, policy)
status: Draft
owner: @ui
created: 2025-12-23
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - UI v2a input routing baseline: tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md
  - UI v6b app lifecycle baseline (focus): tasks/TASK-0065-ui-v6b-app-lifecycle-notifications-navigation.md
  - Clipboard History DSL follow-up: tasks/TASK-0067B-ui-v7b-clipboard-history-dsl-overlay.md
  - Policy as Code (clipboard guards): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Config broker (clipboard budgets): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Testing contract: scripts/qemu-test.sh
---

## Rebase (2026-08-14)

### Verified reality — the baseline is near zero

- Clipboard today is a **host-only** `Mutex<Option<String>>`
  (`userspace/clipboard/src/lib.rs:31`);
  `source/services/clipboardd/src/main.rs` is a **14-line placeholder**; and
  there is **no clipboard module in `source/libs/nexus-wire/src/`** — no OS
  wire protocol exists at all.
- DnD: nothing exists beyond vendored cursor shapes
  (`userspace/ui/cursor/src/hotspot.rs:31-34` — `dnd-none/copy/move/link`).

### Ownership — this task builds the real clipboardd

Successors TASK-0087 (clipboard v3 upgrade) and TASK-0122C (DSL bridge) both
presume a working clipboardd; no other ledger owns it. **This task does.**
Model the service on settingsd (TASK-0072, Done): a real no_std OS entry
point (os-lite), a new `nexus_wire::clipboardd` wire module, and settingsd's
registry/persist pattern — no parallel service shape.

### Boundary fix (docs/dev/ui/windowd-cleanup-map.md:4-9)

windowd = Single Present Authority (compositor SERVICE). The draft's
"drag image overlay (VMO-backed)" inside windowd violates that boundary: the
**drag image is a client/app surface** (the dragging app or the DSL shell app
presents it). windowd gets ONLY:

- DnD **hit-test/routing**: enter/over/leave/drop targeting, and
- **focus semantics** during a drag (no background input leak).

No DnD rendering, no overlay drawing, no clipboard UI in windowd (the history
UI is TASK-0067B's DSL overlay). Never build into a MOVE/DELETE file.

### Process gate

**RFC seed required before implementation**: the clipboard wire protocol
(`nexus_wire::clipboardd`) and the DnD routing ops are new service API/wire
formats → `docs/rfcs/RFC-TEMPLATE.md`, next free number, RFC index update.
`source/libs/**` and `docs/rfcs/**` are approval zones.

### Corrected proof + touched paths

`tests/ui_v7b_host/` never existed, and windowd has no `idl/*.capnp` — ops
live in wire-module libs. Host proofs go to `source/services/clipboardd/tests/`
(service layout: src/ + tests/) and `source/services/windowd/tests/` for DnD
routing. Allowlist below updated.

## Context

To make the system productive, we need interoperable content transfer:

- drag-and-drop with typed payload negotiation,
- a robust clipboard with MIME support and history.

Both must be policy-guarded (focus/foreground constraints) and bounded (budgets).

Screenshot/share is handled separately (v7c).

## Goal

Deliver:

1. DnD protocol + routing in `windowd` (routing/hit-test/focus ONLY — see
   rebase boundary fix):
   - DragSource/DropTarget interfaces
   - global DnD controller: enter/over/leave/drop targeting
   - drag image = client/app surface (bounded VMO); windowd routes it, never
     draws it
   - negotiated pull (`read(mime)` after accept)
2. Clipboard v2 service `clipboardd`:
   - multi-MIME items
   - history ring with configurable size and eviction
   - policy gating: focused/foreground subjects
3. SystemUI integration hooks (minimal):
   - clipboard history popup stub (optional for v7b)
   - full visible DSL clipboard history UI is a follow-up in `TASK-0067B`
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

`source/services/clipboardd/tests/` + `source/services/windowd/tests/`
(corrected 2026-08-14; `tests/ui_v7b_host/` never existed):

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

## Touched paths (allowlist) — corrected 2026-08-14

- `source/services/clipboardd/` (replace the placeholder: src/ + tests/, os-lite, settingsd-pattern)
- `source/libs/nexus-wire/src/clipboardd.rs` (new wire module — approval zone `source/libs/**`)
- `source/services/windowd/` (DnD hit-test/routing + focus semantics ONLY; check the cleanup map first)
- `userspace/clipboard/` (host lib aligns to the same protocol shapes for host tests)
- `docs/rfcs/` (clipboard wire + DnD routing RFC seed — approval zone)
- `source/apps/selftest-client/`
- `docs/dev/ui/patterns/transfer-sharing/drag-and-drop.md` + `docs/dev/ui/patterns/transfer-sharing/clipboard.md`

## Plan (small PRs)

1. dnd IDL + controller + drag image overlay + markers
2. clipboardd + history ring + policy/budgets + markers
3. host tests + OS markers + docs + postflight
