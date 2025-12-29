---
title: TASK-0186 WebView v1.1a (host-first): CSP-Strict sanitizer + webview-core (history/find/session storage) + deterministic tests/goldens
status: Draft
owner: @ui
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - WebView Net v1 host slice (sanitizer/render baseline): tasks/TASK-0176-webview-net-v1a-host-sanitizer-webview-sceneir-goldens.md
  - WebView baseline service (find/text extraction direction): tasks/TASK-0111-ui-v19a-webviewd-sandbox-offscreen.md
  - Renderer abstraction (Scene-IR snapshots): tasks/TASK-0169-renderer-abstraction-v1a-host-sceneir-cpu2d-goldens.md
  - Policy/webpolicy (fixtures-only fetch): tasks/TASK-0177-webview-net-v1b-os-httpstubd-fetchd-downloadd-policy-selftests.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

WebView Net v1 (`TASK-0176/0177`) delivers a deterministic sanitizer+renderer core and a fixture-only fetch path.
WebView v1.1 upgrades the offline WebView experience with:

- navigation history (back/forward/reload + scroll restore),
- find-in-page with deterministic highlights,
- session storage (in-memory, bounded),
- CSP-Strict enforcement integrated into sanitizer decisions.

This is host-first: we prove behavior and goldens deterministically without OS services.
OS/QEMU wiring and file chooser integration is in `TASK-0187`.

## Goal

Deliver:

1. CSP-Strict mode in `userspace/libs/html-sanitizer`:
   - default strict policy:
     - scripts: none
     - styles: inline allowlist only; no URL in style
     - images: `self` and `data:` only (unless relaxed by webpolicy)
     - fonts: `self` only
     - frames: none
   - parse `<meta http-equiv="Content-Security-Policy" ...>` but clamp to equal-or-stricter than default
   - deterministic violation reporting surface (returned in result struct; UI can render dev banner)
   - markers (throttled):
     - `sanitizer: csp strict`
     - `sanitizer: csp violation kind=<...> url=<...>`
2. `userspace/libs/webview-core`:
   - History model:
     - bounded capacity (default 50)
     - `back/current/fwd` with deterministic push/pop rules
     - per-entry `scroll_y` restoration
   - Session storage:
     - per-origin `BTreeMap<String,String>` with global byte cap (e.g. 64 KiB)
     - deterministic eviction/deny-on-exceed behavior (explicit error)
   - Find-in-page:
     - build a deterministic text index over SanitizedDom
     - substring search with explicit case-fold rules
     - stable match ordering and next/prev wrap behavior
     - highlight geometry is produced deterministically (node id + range; Scene-IR overlay handled by UI layer)
   - markers (throttled):
     - `webview: history nav=<back|forward> url=<...>`
     - `webview: find query="<...>" total=<n> cur=<i>`
     - `webview: storage set k=<...>`
3. Control wiring (host-first):
   - update `userspace/ui/controls/webview` to use `webview-core` for:
     - history buttons and scroll restore
     - find UI and highlight overlays
     - session storage events (no JS execution; sample app uses events)
4. Deterministic host tests + goldens (`tests/webview_v1_1_host/`):
   - history: nav chain and scroll restore stable
   - find: total/current stable; next/prev wrap
   - CSP strict: cross-origin resource blocked deterministically; golden render matches “no img” baseline
   - session storage: set/get/remove, per-origin behavior, over-quota deny

## Non-Goals

- Kernel changes.
- Persistent web storage (disk), cookies, or JS execution.
- File chooser integration (v1.1b).

Follow-up:

- WebView v1.2 (persistent history + session restore + CSP report persistence/viewer + cookie jar v0 dev toggle) is tracked as `TASK-0205`/`TASK-0206`.

## Constraints / invariants (hard requirements)

- Determinism: stable ordering, stable tie-breakers, stable folding rules.
- Bounded memory: cap history entries, matches, and session storage bytes.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **YELLOW (CSP + webpolicy interactions)**:
  - v1.1 defaults to strict CSP; webpolicy may relax within an allowlist, but meta CSP cannot loosen beyond policy.
  - Exact precedence order must be documented and covered by tests.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p webview_v1_1_host -- --nocapture`
  - Required: history/find/csp/session-storage tests + at least one golden snapshot.

## Touched paths (allowlist)

- `userspace/libs/html-sanitizer/` (extend)
- `userspace/libs/webview-core/` (new)
- `userspace/ui/controls/webview/` (extend)
- `tests/webview_v1_1_host/` (new)
- `docs/webview/` (added in v1.1b or minimal here)

## Plan (small PRs)

1. CSP strict + deterministic violation reporting + tests
2. webview-core history + session storage + tests
3. find-in-page index + highlight model + tests + a golden render

## Acceptance criteria (behavioral)

- Host tests deterministically prove history/find/storage/CSP-strict behavior and goldens are stable.
