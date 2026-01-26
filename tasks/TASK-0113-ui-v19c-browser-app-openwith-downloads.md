---
title: TASK-0113 UI v19c: Browser app (offline) + downloads shelf + Open With html/svg + OS selftests/postflight
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - WebView daemon: tasks/TASK-0111-ui-v19a-webviewd-sandbox-offscreen.md
  - Content saveAs helper: tasks/TASK-0112-ui-v19b-contentd-saveas-downloads.md
  - MIME registry/Open With: tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md
  - Document picker filters: tasks/TASK-0083-ui-v11c-document-picker-open-save-openwith.md
  - Files app integration: tasks/TASK-0086-ui-v12c-files-app-progress-dnd-share-openwith.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

With `webviewd` available (v19a) and a simple `contentd.saveAs` helper (v19b), we can ship a minimal Browser:

- strictly offline,
- tabs + omnibox for allowed schemes,
- find-in-page,
- downloads shelf for saving `data:` and `content://` items into `state:/Downloads/`,
- Open With wiring for `text/html` (and optionally `image/svg+xml`).

## Goal

Deliver:

1. Browser app `userspace/apps/browser`:
   - omnibox (accepts `content://`, `pkg://`, `data:`, `about:`)
   - back/forward/reload
   - zoom controls (fit/actual/step)
   - tabs (minimal)
   - find-in-page using `webviewd.find/findNext`
   - downloads shelf using `contentd.saveAs` to `state:/Downloads/`
   - markers:
     - `browser: open uri=..`
     - `browser: tab new`
     - `browser: find hits=..`
     - `browser: download saved uri=..`
2. Open With wiring:
   - register Browser as default for `text/html` and optionally `image/svg+xml`
   - Files context menu “Open in Browser” for `.html/.htm/.svg`
   - picker “HTML” filter (if not already present)
3. Host tests and OS selftests + postflight for UI v19.

## Non-Goals

- Kernel changes.
- External networking (http(s) remains blocked).
- Full history persistence (v1 can be in-memory).

## Constraints / invariants

- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Browser must not bypass URL policy; `webviewd` is the enforcement point.
- Bounded downloads (cap bytes; surfaced errors are clear).

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v19_host/`:

- open allowed page and render stable snapshot
- http(s) blocked
- find-in-page stable hit count and next cycling
- data: download saved via saveAs, recents updated
- open-with wiring triggers browser open marker

### Proof (OS/QEMU) — gated

UART markers:

- `webviewd: ready`
- `browser: open uri=`
- `browser: find hits=`
- `browser: download saved uri=`
- `SELFTEST: ui v19 open ok`
- `SELFTEST: ui v19 find ok`
- `SELFTEST: ui v19 download ok`

## Touched paths (allowlist)

- `userspace/apps/browser/` (new)
- SystemUI launcher tile + picker filter + files context menu (minimal)
- `tests/ui_v19_host/`
- `source/apps/selftest-client/`
- `tools/postflight-ui-v19.sh` (delegates)
- `docs/apps/browser.md` (new)

## Plan (small PRs)

1. browser shell + webviewd integration + markers
2. downloads shelf + saveAs integration + recents hook
3. open-with registration + host tests + OS selftests + docs + postflight
