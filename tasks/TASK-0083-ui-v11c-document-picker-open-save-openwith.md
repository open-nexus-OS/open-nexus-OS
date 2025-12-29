---
title: TASK-0083 UI v11c: document picker (open/save) + Open With… + app integration + OS markers/postflight
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
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
   - Open dialog (providers, breadcrumb, grid/list with thumbnails, MIME filter, search via `contentd.query`)
   - Save dialog (filename field, MIME selector, create via `contentd.create` then write to stream)
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

## Non-Goals

- Kernel changes.
- Full cloud provider and auth flows.
- Full file manager.

## Constraints / invariants (hard requirements)

- Stream handles only (no path access in app APIs).
- Deterministic picker behavior for tests (demo-cloud provider is deterministic).
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
- `userspace/apps/notes/` + `userspace/apps/launcher/` (integration)
- `tests/ui_v11_host/`
- `source/apps/selftest-client/`
- `tools/postflight-ui-v11.sh` (delegates)
- `docs/ui/doc-picker.md` (new)

## Plan (small PRs)

1. picker UI overlay + helper API + markers
2. Open With integration + recents hook
3. notes/launcher wiring + host tests + OS selftests + docs + postflight
