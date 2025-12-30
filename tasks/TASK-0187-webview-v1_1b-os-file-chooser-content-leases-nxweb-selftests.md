---
title: TASK-0187 WebView v1.1b (OS/QEMU): file chooser for input[type=file] via content:// + leases/grants + nx-web enhancements + selftests/docs
status: Draft
owner: @ui
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - WebView v1.1 host core: tasks/TASK-0186-webview-v1_1a-host-webview-core-history-find-sessionstorage-csp.md
  - WebView Net v1 OS slice (sample app + nx-web): tasks/TASK-0177-webview-net-v1b-os-httpstubd-fetchd-downloadd-policy-selftests.md
  - Content providers (content:// streams): tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md
  - Document picker overlay + helper API: tasks/TASK-0083-ui-v11c-document-picker-open-save-openwith.md
  - Scoped URI grants (cross-subject): tasks/TASK-0084-ui-v12a-scoped-uri-grants.md
  - Policy capability matrix: tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

WebView v1.1a adds history/find/session storage and CSP-strict host-first. This v1.1b task wires
OS-facing features, primarily:

- `<input type="file">` picking via `content://` URIs (no paths),
- lease semantics (time-bound, deterministic expiry),
- `nx-web` tool enhancements for navigation/find/history,
- and QEMU selftests/markers.

Important: the repo already has a Document Picker direction (`TASK-0083`) and scoped grants (`TASK-0084`).
We should reuse those building blocks instead of introducing a new parallel picker authority unless forced.

## Goal

Deliver:

1. File chooser integration for `<input type="file">`:
   - sanitizer allows `input[type=file]` as a safe control (no JS)
   - WebView control intercepts activation and calls the picker helper (`userspace/ui/picker`) if available
   - returns **`content://...`** URIs to the WebView sample app via an event (`onFileChosen(uris)`)
   - filters derived from the `accept=` attribute deterministically (extension allowlist)
   - markers:
     - `webview: input[type=file] activated`
     - `webview: file chosen uris=<n>`
2. `content://` lease semantics (gated):
   - if `contentd` supports leases/streams already, use it
   - otherwise implement a minimal read-only lease helper consistent with `TASK-0081`/`TASK-0084`:
     - opaque token, deterministic TTL, bounded table, deterministic expiry
   - markers:
     - `content: lease issued token=<...> ttl_s=<n>`
     - `content: lease expired token=<...>`
3. `nx-web` tool enhancements (host/dev tool):
   - open/back/forward/reload/find/history/file-choose commands
   - NOTE: do not rely on running host tools inside QEMU selftests
4. Fixtures:
   - add `form.html` with `<input type="file" accept="image/png">`
   - add `article2.html` for history chaining
   - add `pkg://fixtures/files/` sample files (png/txt) with deterministic hashes
5. OS selftests (bounded):
   - `SELFTEST: webview history back ok`
   - `SELFTEST: webview find ok`
   - `SELFTEST: webview file choose ok`
   - `SELFTEST: webview csp strict ok`
6. Docs:
   - history/find model
   - CSP strict rules and violation behavior
   - file chooser privacy model and `content://` lease semantics

## Non-Goals

- Kernel changes.
- Upload/networking (no form submit).
- Persistent storage for session storage (in-memory only).

## Constraints / invariants (hard requirements)

- No raw path exposure to apps (content:// only).
- Deterministic expiry and deterministic selection ordering.
- No fake success: “file choose ok” only if URI resolves and bytes can be read through the stream API.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **RED (picker reuse vs new service)**:
  - Prefer reuse of `userspace/ui/picker` + Document Picker overlay (`TASK-0083`).
  - Only introduce a dedicated `filepickerd` service if OS UI picker cannot be reused, and document why.

- **YELLOW (grants vs same-subject leases)**:
  - If file chooser returns URIs crossing subjects, it must use `grantsd` (`TASK-0084`).
  - If it stays within the same subject (WebView sample), a simple in-process lease may be sufficient for v1.1, but must not be misrepresented as a security boundary.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p webview_v1_1_host -- --nocapture` (from v1.1a)

- **Proof (QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=195s ./scripts/qemu-test.sh`
  - Required markers:
    - `SELFTEST: webview history back ok`
    - `SELFTEST: webview find ok`
    - `SELFTEST: webview file choose ok`
    - `SELFTEST: webview csp strict ok`

## Touched paths (allowlist)

- `userspace/ui/controls/webview/`
- `userspace/ui/picker/` (reuse if present)
- `source/services/contentd/` (lease/stream helper if needed)
- `source/apps/selftest-client/`
- fixtures under `pkg://fixtures/web/` and `pkg://fixtures/files/`
- `tools/nx-web/` (extend)
- `docs/webview/` + `docs/tools/nx-web.md`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. file input activation → picker integration → content:// URIs event
2. leases/grants wiring (choose minimal consistent approach) + fixtures
3. selftests + docs + marker contract update + nx-web enhancements

## Acceptance criteria (behavioral)

- In QEMU, WebView proves history/find/CSP strict and file chooser returns resolvable content:// URIs deterministically.

