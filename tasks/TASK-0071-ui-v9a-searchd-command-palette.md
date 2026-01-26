---
title: TASK-0071 UI v9a: searchd index + global search / command palette UI + app/settings registration
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - UI v6b app launching (app hits): tasks/TASK-0065-ui-v6b-app-lifecycle-notifications-navigation.md
  - UI v8b shortcuts/overlays baseline (palette UI): tasks/TASK-0070-ui-v8b-wm-resize-move-shortcuts-settings-overlays.md
  - Config broker (search knobs): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Policy as Code (search access): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

UI v9 introduces a “productivity spine”:

- fast global search over apps/settings/files (stub),
- a command palette overlay (Ctrl/Cmd+K),
- wired actions (launch app, open settings route).

This task focuses on search + palette. Persistent preferences store and Settings panels live in v9b (`TASK-0072`).

Scope note:

- Search v2 (richer command-palette surface, deep-links, zero-query springboard, and perf gate wiring) is tracked as
  `TASK-0151` (host-first UI surface + tests) and `TASK-0152` (OS deep-link router + selftests/docs).
- Search v2 backend (real index/analyzers/ranking/sources) is tracked as `TASK-0153` (host-first engine + tests)
  and `TASK-0154` (OS wiring + selftests + docs).

## Goal

Deliver:

1. `searchd` service:
   - in-memory index for apps/settings/files (file source stub)
   - deterministic trigram + BM25-lite ranking
   - IDL for `query/suggest/upsertApp/upsertSetting/upsertFile`
2. SystemUI command palette + global search UI:
   - overlay toggled by shortcut
   - keyboard navigation + activate
3. Registration:
   - `launcher` and at least one app registers itself
   - settings routes are upserted (can be stubs until v9b ships)
4. Host tests and OS/QEMU markers.

## Non-Goals

- Kernel changes.
- Persistent search index on disk.
- Real filesystem indexing (stub only).
- Preferences store and functional settings panels (v9b).

## Constraints / invariants (hard requirements)

- Deterministic ranking and stable tie-breaking rules.
- Bounded memory:
  - cap index size per kind
  - cap keyword lengths and entry counts
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v9a_host/`:

- index 3 items, query variants, assert ranking order is stable
- suggest returns stable items list
- palette navigation simulation selects an entry and triggers expected action hook

### Proof (OS/QEMU) — gated

UART markers (order tolerant):

- `searchd: ready`
- `searchd: indexed (apps=N settings=M files=K)`
- `systemui: palette on`
- `systemui: search hit (kind=app|setting|file id=...)`
- `SELFTEST: ui v9 search ok`

## Touched paths (allowlist)

- `source/services/searchd/` (new)
- `source/services/searchd/idl/search.capnp` (new)
- SystemUI plugins (palette + search UI)
- `userspace/apps/launcher/` + at least one app registration
- `tests/ui_v9a_host/`
- `source/apps/selftest-client/`
- `tools/postflight-ui-v9a.sh` (delegates)
- `docs/dev/ui/search.md`

## Plan (small PRs)

1. searchd: index + ranking + IDL + markers
2. SystemUI: command palette overlay + actions + markers
3. app/route registration + host tests + OS markers + docs + postflight
