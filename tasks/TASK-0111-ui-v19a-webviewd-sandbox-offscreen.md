---
title: TASK-0111 UI v19a: webviewd offscreen WebView (sandboxed, offline) + URL policy + BGRA frames + find/text extraction
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Content provider API: tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md
  - Text stack (find semantics): tasks/TASK-0094-ui-v15a-text-primitives-uax-bidi-hittest.md
  - UI renderer baseline: tasks/TASK-0054-ui-v1a-cpu-renderer-host-snapshots.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We want a WebView that is **strictly offline** in v1:

- allow `about:`, `content://`, `pkg://`, `data:`,
- block `http(s)` and other schemes outright,
- render offscreen into BGRA buffers (VMO-backed),
- provide text extraction for find-in-page and accessibility stubs.

Servo integration is a future path; v19a defines a stable service interface and implements a constrained renderer now.

Browser UX, downloads shelf, and Open With wiring are separate tasks (v19b/v19c).

Scope note (fixtures-only “HTTP”):

- Deterministic fixture fetch over `http://fixture.local/*` (routed to an in-OS stub service, no sockets) is tracked as
  `TASK-0176`/`TASK-0177`. This does **not** change the v1 offline stance: all non-fixture `http(s)` remains blocked.

## Goal

Deliver:

1. `webviewd` service:
   - `create/navigate/resize/frame/input/find/findNext/getHtml/getText` API
   - offscreen rendering into BGRA VMO with bounded frame size
   - per-view worker isolation (thread by default; process-per-view optional and flag-gated)
2. URL policy enforcement:
   - allowlist: `about:blank`, `content://`, `pkg://`, `data:`
   - deny `http(s)` and everything else with clear error codes and markers
3. Find-in-page:
   - `getText` exposes text content (best-effort)
   - `find` + `findNext` deterministic cycling
4. Markers:
   - `webviewd: ready`
   - `webview: create id=..`
   - `webview: nav uri=..`
   - `webview: frame ts=..`
   - `policy: webview sandbox on`
   - `policy: webview block http`
5. Host tests for URL policy, rendering snapshot stability, and find behavior.

## Non-Goals

- Kernel changes.
- External networking.
- Full HTML/CSS/JS engine.
- Real accessibility tree (text extraction only v1).

## Constraints / invariants (hard requirements)

- Strict deny-by-default URL policy.
- Bounded rendering:
  - clamp size by `max_bitmap_bytes`,
  - cap DOM/node counts (if applicable),
  - cap input event queue length.
- Deterministic snapshots for fixture pages.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v19a_host/`:

- navigation policy:
  - `http://example.com` rejected deterministically
  - `pkg://...` and `data:` accepted
- rendering:
  - render `pkg://tests/web/box.html` to PNG; snapshot matches golden (pixel-exact preferred; SSIM threshold if needed)
- find:
  - `find("lorem")` returns stable hit count; `findNext` cycles deterministically

### Proof (OS/QEMU) — gated

UART markers:

- `webviewd: ready`

## Touched paths (allowlist)

- `source/services/webviewd/` (new)
- `source/services/webviewd/idl/webview.capnp` (new)
- `tests/ui_v19a_host/`
- `docs/ui/webview.md` (new)

## Plan (small PRs)

1. webviewd IDL + URL policy + markers
2. offscreen rendering implementation (subset) + frame VMO plumbing
3. text extraction + find/findNext
4. host tests + docs
