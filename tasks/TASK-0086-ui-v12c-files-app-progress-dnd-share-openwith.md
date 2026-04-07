---
title: TASK-0086 UI v12c: Files app (grid/list/search/thumbnails) + progress UI + DnD + Open With… + Share integration + OS proofs
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - DSL query posture: docs/dev/dsl/db-queries.md
  - Files usage posture: docs/dev/ui/collections/files.md
  - UI lazy loading posture: docs/dev/ui/collections/lazy-loading.md
  - DSL app platform: tasks/TASK-0122B-dsl-app-platform-v1-shell-routing-launch-contract.md
  - DSL app integration kit: tasks/TASK-0122C-dsl-app-integration-kit-v1-picker-clipboard-share-print.md
  - DSL syntax/layout convention: tasks/TASK-0075-dsl-v0_1a-syntax-ir-cli.md
  - QuerySpec v1 foundation: tasks/TASK-0078B-dsl-v0_2b-queryspec-v1-foundation-service-gated-paging-hash.md
  - DSL query objects: tasks/TASK-0274-dsl-v0_2c-db-query-objects-builder-defaults-paging-deterministic.md
  - UI layout pipeline contract: docs/dev/ui/foundations/layout/layout-pipeline.md
  - Document Access foundations: tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md
  - Thumbnailer/recents: tasks/TASK-0082-ui-v11b-thumbnailer-recents.md
  - Document picker/open-with: tasks/TASK-0083-ui-v11c-document-picker-open-save-openwith.md
  - Scoped grants: tasks/TASK-0084-ui-v12a-scoped-uri-grants.md
  - FileOps/Trash: tasks/TASK-0085-ui-v12b-fileops-trash-services.md
  - DnD controller: tasks/TASK-0067-ui-v7b-dnd-clipboard-v2.md
  - Share sheet: tasks/TASK-0068-ui-v7c-screenshot-screencap-share-sheet.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

With content providers + thumbnails + recents + grants + file ops in place, we can ship a usable Files app.
This task is user-facing and includes end-to-end OS/QEMU markers.

## Goal

Deliver:

1. Files app (`userspace/apps/files`):
   - visible UI is authored directly in the DSL:
     - `ui/pages/FilesPage.nx`
     - `ui/components/**.nx`
     - `ui/composables/**.store.nx` for pure state/search/selection logic
     - `ui/services/**.service.nx` for effect-side adapters to content/fileops/grants/share
   - `FilesPage.nx` may colocate `Store`, `Event`, `reduce`, `@effect`, and `Page` while the app is still small
   - provider sidebar (state/pkg/mem/demo-cloud)
   - breadcrumbs and search (delegates to `contentd.query`)
   - provider listings, folder listings, search/filter state, and curated views (Recent/Downloads/Pictures/Trash) should
     prefer typed QuerySpec builders in DSL/composable code, then execute them only via `contentd.query(...)` or the
     owning domain service
   - grid/list view with thumbnails via `thumbd`
   - list/grid shells should reuse deterministic width-bucket measurement and stable item anchors across resize/filter changes
   - multi-select and actions toolbar (new folder, rename, delete→trash, restore, open-with, share)
   - polish views (v1.2):
     - Home (curated roots), Recent (from `recentsd`), Downloads/Pictures (provider-backed folders), Trash (from `trashd`)
     - info pane shows bounded metadata (uri, mime, size, modified) and active grant presence (if any)
2. Operations + progress UI:
   - background queue via `fileopsd` and progress drawer with cancel
3. Scoped grants UX:
   - when dragging/dropping or “Open With…”, Files issues a grant token for the target subject
4. DnD + Share integration:
   - DnD from Files to apps emits `text/uri-list` plus grant token bundle (v1)
   - Share sheet can share selected URIs (v1 broker in `TASK-0068`), and can later be upgraded to Share v2 intents (`TASK-0126`/`TASK-0127`/`TASK-0128`)
5. Host tests (model-level + DSL UI) and OS selftests + postflight.

## Non-Goals

- Kernel changes.
- Full cloud integration (demo-cloud remains deterministic stub).

## Constraints / invariants (hard requirements)

- No direct filesystem path access; everything uses content URIs and stream handles.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Bounded operations and UI lists:
  - cap visible item count per view,
  - cap concurrent file operations.
- QuerySpec is the preferred contract for Files data views that need filtering, ordering, or paging; command flows such
  as open/share/delete/restore remain domain service actions rather than generic queries.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v12c_host/`:

- Files model can list providers, filter/search, and start operations with progress rows (mocked services)
- DnD offer includes URI list and a grant token when crossing subjects (mocked grantsd)
- Files DSL UI snapshots/interactions are deterministic under host fixtures

### Proof (OS/QEMU) — gated

UART markers:

- `grantsd: ready`
- `fileopsd: ready`
- `trashd: ready`
- `SELFTEST: ui v12 copy ok`
- `SELFTEST: ui v12 trash/restore ok`
- `SELFTEST: ui v12 dnd ok`
- `SELFTEST: ui v12 share ok`

## Touched paths (allowlist)

- `userspace/apps/files/` (new)
- `userspace/apps/files/ui/` (DSL pages/components/composables/services)
- SystemUI launcher/settings integration (add Files tile; default apps UI if needed)
- `tests/ui_v12c_host/`
- `source/apps/selftest-client/`
- `tools/postflight-ui-v12.sh` (delegates)
- `docs/dev/ui/collections/files.md` (new)

## Plan (small PRs)

1. Files UI skeleton (provider sidebar + list/grid + thumbnails)
2. ops/progress drawer wiring via fileopsd
3. trash/restore wiring
4. DnD + Open With + Share wiring (gated on services)
5. host tests + OS selftests + docs + postflight
