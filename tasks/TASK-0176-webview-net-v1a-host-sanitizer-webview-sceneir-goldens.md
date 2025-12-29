---
title: TASK-0176 WebView Net v1a (host-first): HTML sanitizer v2 + SanitizedDOM IR → Scene-IR WebView control + fixtures + golden snapshots
status: Draft
owner: @ui
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - WebView baseline (offline): tasks/TASK-0111-ui-v19a-webviewd-sandbox-offscreen.md
  - Downloads helper baseline (saveAs): tasks/TASK-0112-ui-v19b-contentd-saveas-downloads.md
  - Renderer abstraction (Scene-IR + cpu2d goldens): tasks/TASK-0169-renderer-abstraction-v1a-host-sceneir-cpu2d-goldens.md
  - Renderer OS wiring (windowd): tasks/TASK-0170-renderer-abstraction-v1b-os-windowd-wiring-textshape-perf-markers.md
  - L10n/font fallback (optional for text): tasks/TASK-0174-l10n-i18n-v1a-host-core-fluent-icu-fontsel-goldens.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

`TASK-0111` defines a strictly offline WebView service. This prompt adds a deterministic “networking slice”
but still requires OS/QEMU to remain offline. The safest split is:

- v1a: host-first sanitizer + deterministic DOM subset + Scene-IR rendering control + goldens
- v1b: OS services (httpstubd/fetchd/downloadd) and policy caps (separate task)

This task delivers the **content pipeline** and rendering, independent of how bytes are fetched.

## Goal

Deliver:

1. HTML sanitizer v2 (`userspace/libs/html-sanitizer` or equivalent):
   - input: HTML (+ optional CSS)
   - output: `SanitizedDom` IR (block/inline/img/link)
   - allowlist:
     - tags: `p,div,span,h1..h3,ul,li,strong,em,a,img`
     - attributes: `href/src/alt/title` (no event handlers)
     - styles (subset): `color`, `background-color`, `font-weight`, `font-style` via a deterministic tokenizer
   - URL rules:
     - reject unknown schemes
     - reject inline JS/event handlers
     - resolve relative URLs against base
   - markers (throttled):
     - `sanitizer: ok nodes=<n> imgs=<n> links=<n>`
     - `sanitizer: blocked tag=script`
2. WebView control renderer (`userspace/ui/controls/webview` or equivalent):
   - converts SanitizedDom → Scene-IR:
     - block flow layout (v1)
     - inline text layout (v1; no JS)
     - images drawn with nearest sampling
     - links underlined and emit “navigate” events (host tests use mocks)
   - a11y roles for headings/links (host mock)
   - markers (throttled):
     - `webview: render nodes=<n>`
3. Fixtures:
   - deterministic HTML/CSS/PNG fixtures (repo-tracked assets):
     - `index.html`, `article.html`, `styles.css`, `logo.png`, `doc.bin` (128 KiB)
4. Host golden snapshot tests:
   - render `index.html` → PNG matches golden
   - sanitizer removes scripts and filters CSS deterministically
   - link/image URL validation deterministic

Follow-up note (v1.1 features):

- History/navigation, find-in-page, session storage, and CSP-Strict are tracked as `TASK-0186`/`TASK-0187`.

## Non-Goals

- Kernel changes.
- Any real networking or HTTP client behavior (v1b).
- JavaScript execution.
- Full CSS layout engine (v1 block/inline only).

## Constraints / invariants (hard requirements)

- Determinism: stable output DOM IR and stable Scene-IR rendering results.
- Bounded parsing: cap input bytes, cap nodes, cap CSS tokens.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **YELLOW (overlap with webviewd v19a)**:
  - This task should implement reusable sanitizer + renderer pieces that `webviewd` can embed later,
    rather than creating a parallel “web engine”.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p webview_net_v1_host -- --nocapture`
  - Required:
    - sanitizer tests (blocked tags, filtered CSS, URL scheme rejection)
    - renderer snapshot golden for fixture HTML

## Touched paths (allowlist)

- `userspace/libs/html-sanitizer/` (new)
- `userspace/ui/controls/webview/` (new)
- `userspace/fixtures/web/` (or `assets/fixtures/web/`, choose a canonical fixtures path)
- `tests/webview_net_v1_host/` (new)
- `docs/webview/overview.md` (added in v1b or minimal here)

## Plan (small PRs)

1. sanitizer v2 + deterministic fixtures + tests
2. WebView control: SanitizedDom → Scene-IR + golden snapshots
3. docs stubs for integration and limitations

## Acceptance criteria (behavioral)

- Host goldens are stable; sanitizer decisions are deterministic and auditable.
