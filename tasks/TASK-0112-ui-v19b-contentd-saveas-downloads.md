---
title: TASK-0112 UI v19b: contentd saveAs helper (data:/content:// → state://Downloads) + recents integration + tests
status: Draft
owner: @platform
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Content providers: tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md
  - Scoped grants (optional cross-subject): tasks/TASK-0084-ui-v12a-scoped-uri-grants.md
  - Recents service: tasks/TASK-0082-ui-v11b-thumbnailer-recents.md
  - Persistence (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Policy as Code (download allowlist): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
---

## Context

Browser v1 needs a minimal “downloads shelf” even without networking:

- `data:` URLs (generated content) and `content://` URIs should be saveable to `state://Downloads/`.

Rather than duplicating copy logic in apps, add a small `contentd.saveAs` helper:

- resolves an input URI (including `data:`),
- writes bytes into a destination provider (`state`),
- returns the output URI and bytes written.

Webviewd and browser UX are separate tasks.

## Goal

Deliver:

1. Extend `contentd` with `saveAs(uri, destParent, name) -> outUri, bytes`:
   - supports `data:` and `content://` inputs
   - writes into `state://` destination
   - bounded copy (chunking; max bytes)
2. Policy gates:
   - allow `data:` saveAs only for permitted subjects (default: Browser/SystemUI; tests allow selftest)
   - deny cross-app state subtrees unless a grant token is present (if applicable)
3. Recents integration:
   - successful saveAs adds entry to recents (uri + mime)
4. Host tests for deterministic behavior.

## Non-Goals

- Kernel changes.
- Full download manager.

## Constraints / invariants

- Deterministic output bytes and naming rules for fixtures.
- Bounded copy:
  - cap max bytes per save,
  - cap chunk size.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v19b_host/`:

- saveAs `data:` PNG to state Downloads returns expected bytes count and a resolvable output URI
- saveAs `content://` to state downloads succeeds and is byte-identical
- recents updated
- policy deny case blocks saveAs deterministically when disabled

## Touched paths (allowlist)

- `source/services/contentd/` (extend)
- `tests/ui_v19b_host/`
- `docs/platform/content.md` (extend with saveAs examples)

## Plan (small PRs)

1. contentd saveAs implementation + bounds + marker logs (if needed)
2. policy gates + recents hook
3. host tests + docs update
