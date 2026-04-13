---
title: TASK-0083 UI v11c: document picker (open/save) + Open With… + app integration + OS markers/postflight
status: Draft
owner: @ui
created: 2025-12-23
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - DSL query posture: docs/dev/dsl/db-queries.md
  - Document picker usage posture: docs/dev/ui/system-experiences/doc-picker.md
  - DSL app integration kit: tasks/TASK-0122C-dsl-app-integration-kit-v1-picker-clipboard-share-print.md
  - DSL syntax/layout convention: tasks/TASK-0075-dsl-v0_1a-syntax-ir-cli.md
  - QuerySpec v1 foundation: tasks/TASK-0078B-dsl-v0_2b-queryspec-v1-foundation-service-gated-paging-hash.md
  - DSL query objects: tasks/TASK-0274-dsl-v0_2c-db-query-objects-builder-defaults-paging-deterministic.md
  - MIME + content providers: tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md
  - Thumbnailer + recents: tasks/TASK-0082-ui-v11b-thumbnailer-recents.md
  - WM/modal overlays baseline: tasks/TASK-0074-ui-v10b-app-shell-adoption-modals.md
  - App lifecycle launch (Open With target): tasks/TASK-0065-ui-v6b-app-lifecycle-notifications-navigation.md
  - Screenshot/share (optional destination): tasks/TASK-0068-ui-v7c-screenshot-screencap-share-sheet.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

With MIME/content providers (v11a) and thumb/recents (v11b), we can ship the user-facing slice:

- a SystemUI document picker overlay usable by apps (open/save),
- Open With… association flow,
- app integration (notes open/save by URI),
- end-to-end OS markers + postflight.

## Goal

Deliver:

1. SystemUI Document Picker overlay:
   - visible picker UI is authored in the DSL and mounted as a SystemUI overlay
   - canonical placement may be either:
     - `userspace/systemui/dsl/pages/DocumentPicker.nx`, or
     - a shared picker package with `ui/pages/DocumentPickerPage.nx`
   - page files follow the canonical `Store` + `Event` + `reduce` + `@effect` + `Page` structure from `TASK-0075`
   - Open dialog (providers, breadcrumb, grid/list with thumbnails, MIME filter, search via `contentd.query`)
   - picker search/filter/order state should build a typed QuerySpec in DSL/composable code and execute it only through
     `contentd.query(...)`, so provider-backed views keep deterministic ordering, paging, and virtual-list friendliness
   - Save dialog (filename field, MIME selector, create via `contentd.create` then write to stream)
   - Folder dialog (select folder) for “pick destination folder” flows (returns a folder URI)
   - “Remember access” UX (gated on `/state`):
     - checkbox requests a **persistable scoped grant** for the selected URI
     - delegates to `grantsd` (see `TASK-0084`), and must not invent a parallel grant store
   - keyboard and basic gesture navigation
   - returns `(uri, mime)` to caller
   - markers:
     - `docpicker: open (mode=open|save, mime=...)`
     - `docpicker: result (uri=..., mime=...)`
2. `userspace/ui/picker` helper API:
   - `open_file(mime_filter)` and `save_file(suggest_name,mime)`
3. Open With… integration:
   - list apps from `mimed.queryByMime`
   - launch selected app via `appmgrd.Launch(appId, args={openUri})`
4. App wiring:
   - `notes` supports `openUri` arg and can open/save via picker
   - `launcher` can show recents grid (optional) and registers supported mimes
   - markers:
     - `notes: open uri=...`
     - `notes: saved uri=...`
5. Host tests and OS selftests + postflight.
   - host tests include DSL picker snapshots/interactions in addition to picker model tests

## Non-Goals

- Kernel changes.
- Full cloud provider and auth flows.
- Full file manager.

## Constraints / invariants (hard requirements)

- Stream handles only (no path access in app APIs).
- Deterministic picker behavior for tests (demo-cloud provider is deterministic).
- picker query state should remain a pure QuerySpec value; execution stays service-gated and must not become direct DB/file
  access from UI code
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v11_host/`:

- picker model:
  - filter by MIME, search prefix works
  - save creates file and writes deterministic bytes via stream
- Open With:
  - association list matches mimed registry
  - setDefault affects UI selection deterministically
- recents integration:
  - successful open/save adds entries
- picker DSL UI snapshots/interactions are deterministic under host fixtures

### Proof (OS/QEMU) — gated

UART markers:

- `mimed: ready`
- `contentd: ready`
- `thumbd: ready`
- `recentsd: ready`
- `docpicker: result (uri=..., mime=...)`
- `SELFTEST: ui v11 open ok`
- `SELFTEST: ui v11 save ok`
- `SELFTEST: ui v11 openwith ok`
- `SELFTEST: ui v11 recents ok`

## Touched paths (allowlist)

- SystemUI plugins (docpicker overlay)
- `userspace/ui/picker/` (new)
- `userspace/ui/picker/ui/` (DSL pages/components/composables/services)
- `userspace/apps/notes/` + `userspace/apps/launcher/` (integration)
- `tests/ui_v11_host/`
- `source/apps/selftest-client/`
- `tools/postflight-ui-v11.sh` (delegates)
- `docs/dev/ui/system-experiences/doc-picker.md` (new)

## Plan (small PRs)

1. picker UI overlay + helper API + markers
2. Open With integration + recents hook
3. notes/launcher wiring + host tests + OS selftests + docs + postflight
