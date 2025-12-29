---
title: TASK-0177 WebView Net v1b (OS/QEMU): httpstubd + fetchd (policy-gated) + downloadd quotas/progress + sample app + nx-web + selftests/docs
status: Draft
owner: @platform
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - WebView baseline (offline URL policy): tasks/TASK-0111-ui-v19a-webviewd-sandbox-offscreen.md
  - WebView Net v1a sanitizer/render core: tasks/TASK-0176-webview-net-v1a-host-sanitizer-webview-sceneir-goldens.md
  - Downloads helper baseline (saveAs to state): tasks/TASK-0112-ui-v19b-contentd-saveas-downloads.md
  - Policy v1.1 caps/scopes (prefer reuse): tasks/TASK-0167-policy-v1_1-host-scoped-grants-expiry-enumeration.md
  - SDK manifest lint tooling (webpolicy.json validation; optional): tasks/TASK-0165-sdk-v1-part2a-devtools-lints-pack-sign-ci.md
  - Persistence (/state downloads): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

`TASK-0111` mandates “strictly offline” WebView in v1 (block http(s)). We still want a deterministic “networking slice”
for development and UI plumbing without enabling real networking.

This task implements:

- `httpstubd`: deterministic fixture-backed “HTTP responder” (no sockets; pure service)
- `fetchd`: policy-gated fetch that routes `http://fixture.local/...` to httpstubd
- `downloadd`: quotas/progress pipeline that writes to `/state/Downloads` when `/state` exists

It does **not** enable real networking; “devnet” is a future follow-up once networking and policy are real.

Follow-up note (real devnet/TLS):

- A dev-only real HTTP(S) backend for `fetchd` (resolverd/netd/tlsd, trust roots, pinning) is tracked as `TASK-0193` (host-first) and `TASK-0194` (OS-gated).

## Goal

Deliver:

1. Policy surface:
   - caps:
     - `net.http.fetch` (fixtures only)
     - `net.http.devnet` (dev-only; must remain disabled in v1)
     - `downloads.write`
   - `webpolicy.json` per-app policy:
     - allowed hosts list includes `fixture.local`
     - allowed schemes includes `http` (fixtures only)
     - max download bytes
   - enforcement:
     - `fetchd` calls `policyd.require(appId, cap, scope="http://<host>/*")`
     - wildcard scopes remain denied unless dev-mode is enabled (policy v1.1)
2. `httpstubd` service:
   - fetch(req) returns fixture bytes from `pkg://fixtures/web/`
   - deterministic headers (no Date), deterministic ETag, gzip disabled
   - markers:
     - `httpstubd: ready`
     - `httpstub: fetch 200 url=/index.html`
3. `fetchd` service:
   - validates scheme/host against webpolicy.json
   - enforces response size caps and redirect rules
   - routes `http://fixture.local/*` → httpstubd
   - markers:
     - `fetchd: ready`
     - `fetchd: allow app=<id> host=fixture.local`
     - `fetchd: deny host=<host>`
4. `downloadd` service:
   - enqueue(url,name) returns id
   - progress() exposes bounded progress state
   - uses fetchd underneath
   - writes to `state:/Downloads/...` only if `/state` exists; otherwise explicit `stub/placeholder` (no “done ok”)
   - markers:
     - `downloadd: ready`
     - `downloadd: start url=<url>`
     - `downloadd: done file=<name> bytes=<n>`
5. Sample app + QS entry:
   - `webview-sample` loads `http://fixture.local/index.html` via fetchd, sanitizes, renders via Scene-IR (v1a)
   - supports “download” link that triggers downloadd for `doc.bin`
6. Host CLI `nx-web` (host-first tool):
   - fetch/sanitize/render/download helpers (for development)
   - NOTE: must not be used inside QEMU selftests
7. OS selftests (bounded):
   - `SELFTEST: web fetch ok`
   - `SELFTEST: web sanitize/render ok`
   - `SELFTEST: web download ok` (only if `/state` exists; otherwise explicit `stub/placeholder`)
8. Docs:
   - security model (offline fixtures; policy gating; sanitizer)
   - limitations (no JS, no real HTTP)

## Non-Goals

- Kernel changes.
- Real external networking in OS/QEMU.
- A full browser with tabs/history/download shelf UI (separate tasks).

Follow-up:

- WebView v1.2 extends downloads with pause/resume semantics (devnet-gated) and adds persistent history/session/CSP viewer. Tracked as `TASK-0205`/`TASK-0206`.

## Constraints / invariants (hard requirements)

- Offline & deterministic: no sockets, no system time headers, deterministic fixture mapping.
- No fake success: if `/state` missing, downloads cannot claim persistence.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **RED (URL policy contradiction)**:
  - WebView v1 is offline and blocks http(s). This task only allows `http://fixture.local/*` via fetchd+httpstubd
    as a deterministic fixture mechanism. All other http(s) remains denied.

- **RED (/state gating)**:
  - download persistence requires `TASK-0009`. Without it, download “done” must be placeholder.

- **YELLOW (webpolicy lint tooling)**:
  - validating `webpolicy.json` in `nx-manifest-lint` depends on SDK tooling (`TASK-0165`). Until then, fetchd must validate schema itself.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p webview_net_v1_host -- --nocapture` (from v1a)
  - plus any host tests added for fetchd/downloadd policy logic (optional)

- **Proof (QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=195s ./scripts/qemu-test.sh`
  - Required markers:
    - `httpstubd: ready`
    - `fetchd: ready`
    - `downloadd: ready`
    - `SELFTEST: web fetch ok`
    - `SELFTEST: web sanitize/render ok`
    - `SELFTEST: web download ok` (only if `/state` exists; otherwise explicit placeholder)

## Touched paths (allowlist)

- `source/services/httpstubd/` (new)
- `source/services/fetchd/` (new)
- `source/services/downloadd/` (new)
- `userspace/apps/webview-sample/` (new)
- `tools/nx-web/` (new; host-only)
- `source/apps/selftest-client/`
- `docs/webview/` + `docs/networking/policy.md`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. httpstubd + fetchd (fixture routing + policy gates) + markers
2. downloadd (quotas/progress) + markers (persistence gated)
3. webview-sample app + QS entry + selftests
4. nx-web tool + docs + marker contract update

## Acceptance criteria (behavioral)

- In QEMU, fixture fetch + sanitize/render is proven deterministically; downloads are proven when `/state` exists and otherwise explicitly skipped.
