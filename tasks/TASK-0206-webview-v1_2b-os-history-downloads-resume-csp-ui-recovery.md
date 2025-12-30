---
title: TASK-0206 WebView v1.2b (OS/QEMU): persistent history + session restore + downloadd pause/resume (devnet-gated) + content:// leases + CSP report viewer + selftests/docs
status: Draft
owner: @ui
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - WebView v1.2 host substrate: tasks/TASK-0205-webview-v1_2a-host-history-session-csp-cookies.md
  - WebView v1.1 OS wiring (file chooser/leases): tasks/TASK-0187-webview-v1_1b-os-file-chooser-content-leases-nxweb-selftests.md
  - WebView Net v1 OS services (httpstubd/fetchd/downloadd): tasks/TASK-0177-webview-net-v1b-os-httpstubd-fetchd-downloadd-policy-selftests.md
  - Devnet real HTTP(S) (OS-gated): tasks/TASK-0194-networking-v1b-os-devnet-gated-real-connect.md
  - Persistence substrate (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Policy cap matrix baseline: tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

WebView v1.2a defines deterministic history/session/CSP formats host-first.
This task wires them into OS/QEMU with strict gating:

- history persistence requires `/state`,
- download resume requires a real HTTP(S) backend, which is **devnet-gated** (see `TASK-0194`),
- fixture-only downloads remain available via `TASK-0177` but do not need Range/resume.

## Goal

Deliver:

1. Persistent history integration:
   - store under `state:/web/` (exact filenames documented)
   - wire WebView control:
     - record visit on navigation commit
     - update scroll with bounded debounce
   - support:
     - recent/search/clear/export
   - markers:
     - `webhistory: record url=<...> id=<n>`
     - `webhistory: export bytes=<n>`
2. Session restore/crash recovery:
   - persist `state:/web/session.json` bounded
   - restore prompt on startup (WebView sample or browser shell)
   - markers:
     - `webview: session save tabs=<n>`
     - `webview: session restore`
3. `downloadd` resume semantics:
   - base downloadd pipeline is `TASK-0177`
   - add pause/resume:
     - **only** when using devnet real backend (HTTP Range supported) and policy allows
     - if Range unsupported: deterministic restart from 0 with explicit reason
   - enforce quotas on `state:/Downloads/**`
   - `content://` lease for completed downloads:
     - reuse existing lease/grant direction from `TASK-0187` (no raw paths)
   - markers:
     - `download: progress id=<...> pct=<n>`
     - `download: resumed id=<...> off=<n>`
     - `download: done id=<...> bytes=<n>`
4. CSP report persistence + viewer:
   - write CSP violation events to `state:/csp/reports.jsonl` (gated on `/state`)
   - Settings page to view/filter/export/clear
   - export path under `state:/exports/`
   - markers:
     - `ui: csp reports open`
     - `ui: csp export bytes=<n>`
     - `ui: csp clear ok`
5. Cookie jar v0:
   - remains **disabled by default** and is dev-only
   - OS enablement (if any) must be gated behind devnet and config
6. `nx-web` extensions:
   - history recent/search/export/clear
   - download list/pause/resume/open/clear
   - NOTE: QEMU selftests must not rely on running host tools inside QEMU
7. OS selftests (bounded, deterministic):
   - session restore:
     - `SELFTEST: web session restore ok`
   - downloads:
     - fixture download ok (from v1): `SELFTEST: web download ok` (already in `TASK-0177`)
     - devnet resume path (only if devnet enabled/unblocked): `SELFTEST: web download resume ok`
   - export:
     - `SELFTEST: web export ok` (history + CSP exports exist; gated on `/state`)
   - clear history:
     - `SELFTEST: web history clear ok`

## Non-Goals

- Kernel changes.
- Online/external network access by default.
- Full browser UX (tabs UI etc.) beyond the minimal restore prompt.

## Constraints / invariants (hard requirements)

- `/state` gating: without `TASK-0009`, persistence/export must be disabled or explicitly `stub/placeholder`.
- devnet gating: Range/resume must not claim success unless real backend is enabled (`TASK-0194`).
- No fake success: selftests must validate outcomes by reading service state / file existence, not log greps.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p webview_v1_2_host -- --nocapture` (from v1.2a)

- **Proof (QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=200s ./scripts/qemu-test.sh`
  - Required markers:
    - `SELFTEST: web session restore ok`
    - `SELFTEST: web export ok` (only if `/state` exists; otherwise explicit placeholder)
    - `SELFTEST: web history clear ok`
  - Optional (only when devnet enabled/unblocked):
    - `SELFTEST: web download resume ok`

## Touched paths (allowlist)

- `userspace/libs/webhistory/`
- `userspace/ui/controls/webview/` + `userspace/libs/webview-core/`
- `source/services/downloadd/` (resume + leases)
- `source/services/fetchd/` (backend selection already tracked; consumption here)
- SystemUI Settings pages (CSP viewer) + downloads UI (optional)
- `tools/nx-web/`
- `source/apps/selftest-client/`
- `schemas/webview_v1_2.schema.json`
- `docs/webview/` + `docs/downloads/` + `docs/tools/nx-web.md`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. history persistence wiring + selftests
2. session restore wiring + selftests
3. CSP report log + viewer + export/clear + selftests (gated)
4. downloadd resume semantics (devnet-gated) + content leases + selftests
5. nx-web extensions + docs + marker contract update

## Acceptance criteria (behavioral)

- In QEMU, WebView history/session persistence and CSP report viewing/export behave deterministically when `/state` exists.
- Download resume is only claimed when devnet is enabled and Range support is proven; otherwise behavior is explicit and deterministic.

