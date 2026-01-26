---
title: TASK-0152 Search v2 UI (OS/QEMU): deep-link router + intents/open-with + selftests/postflight + docs (+ perf gate wiring)
status: Draft
owner: @ui
created: 2025-12-26
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Search v2 UI host slice: tasks/TASK-0151-search-v2-ui-host-command-palette-model-a11y.md
  - Search backend (searchd): tasks/TASK-0071-ui-v9a-searchd-command-palette.md
  - Search v2 backend (OS wiring): tasks/TASK-0154-search-v2-backend-os-persistence-selftests-postflight-docs.md
  - App lifecycle/launching: tasks/TASK-0065-ui-v6b-app-lifecycle-notifications-navigation.md
  - MIME/Open-With substrate: tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md
  - Recents substrate: tasks/TASK-0082-ui-v11b-thumbnailer-recents.md
  - Intents v2 (share/dispatch): tasks/TASK-0126-share-v2a-intentsd-registry-dispatch-policy-host.md
  - Policy (capability matrix): tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Perf tracer + hooks (gated): tasks/TASK-0143-perf-v1a-perfd-frame-trace-metrics.md
  - Perf instrumentation (gated): tasks/TASK-0144-perf-v1b-instrumentation-hud-nx-perf.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

`TASK-0151` ships the host-first Search v2 command palette UI surface. This task wires OS execution:

- deep-link routing to Settings/App launch/Open-With,
- policy checks for execution,
- bounded OS selftests with deterministic markers,
- docs and a postflight that delegates to canonical proofs.

Perf “gates” are included as **optional wiring** and must be explicitly gated on `perfd` tasks.

Backend note:

- Search v2 UI execution assumes `searchd.query/suggest` are real. That requirement is tracked in `TASK-0153/0154`.

## Goal

Deliver:

1. SystemUI execution router (`searchexec`)
   - routes result URIs:
     - `setting://...` → open Settings page + scroll/highlight target
     - `app://<id>` → launch/focus app
     - `file://...` / `content://...` → `mimed.openWithDefault` or chooser when ambiguous
     - otherwise → deterministic “Unsupported search result” toast
   - markers:
     - `searchexec: open setting=<uri>`
     - `searchexec: launch app=<id>`
     - `searchexec: open file=<uri>`
2. Zero-query springboard (OS data sources)
   - combine:
     - `recentsd` (recent files)
     - app list from bundle manager (recently used/installed; deterministic tie-break)
     - small “popular settings” list (static deterministic seed for v2)
3. Policy
   - require `search.query` (or equivalent cap) before execution
   - no network calls
   - cache suggestions in RAM only; no extra persistence beyond existing services
4. OS selftests (bounded)
   - open palette, type query, activate result, verify markers:
     - `SELFTEST: search ui setting ok`
     - `SELFTEST: search ui app ok`
     - `SELFTEST: search ui zeroquery ok`
5. Tooling
   - `nx search ui open|close|zeroquery` minimal hooks (optional if `nx` scaffolding exists; otherwise keep as a follow-up)
6. Perf wiring (gated)
   - if `perfd` exists:
     - start/stop a `search_palette` session on open/close
     - emit a deterministic `perf: gate search_palette ok|fail` marker only when the gate is truly evaluated

## Non-Goals

- Kernel changes.
- Remote search or network-backed sources.
- Building a new `intentsd` if it is not yet present (prefer `mimed` + existing launch/route hooks).

## Constraints / invariants (hard requirements)

- Deterministic behavior and bounded timeouts in QEMU.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake-success: do not print “ok” markers unless the routed action actually ran.

## Red flags / decision points (track explicitly)

- **YELLOW (intents/open-with availability)**:
  - If `intentsd` is not shipped on OS yet, prefer the `mimed.openWithDefault` path.
  - If `mimed` is not shipped, the file/open-with portion must be explicitly stubbed with `stub/placeholder` markers.

- **RED (perf gates require perfd)**:
  - “perf gate” output must be gated on `TASK-0143`/`TASK-0144`. Until then, do not emit `perf: gate ... ok`.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - Command(s):
    - `cargo test -p search_v2_ui_host -- --nocapture`

- **Proof (QEMU)**:
  - Command(s):
    - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=185s ./scripts/qemu-test.sh`
  - Required markers (to be added to `scripts/qemu-test.sh` expected list):
    - `searchui: open`
    - `searchexec: open setting=setting://display/dark-mode` (or the chosen canonical URI)
    - `SELFTEST: search ui setting ok`
    - `SELFTEST: search ui app ok`
    - `SELFTEST: search ui zeroquery ok`

## Touched paths (allowlist)

- `userspace/systemui/` (palette execution router)
- `source/apps/selftest-client/`
- `scripts/qemu-test.sh` (marker contract update)
- `tools/postflight-search-v2-ui.sh` (delegates)
- `docs/search/ui.md`
- `docs/search/integration.md`
- `docs/dev/ui/testing.md` (search UI section)

## Plan (small PRs)

1. Router implementation + policy checks + markers
2. Zero-query OS sources wiring
3. Selftests + marker contract + docs + postflight
4. Perf gate wiring only after perfd exists

## Acceptance criteria (behavioral)

- In QEMU, palette opens, results activate, and deep-links execute with deterministic markers.
- Postflight delegates to canonical proofs (host tests + `scripts/qemu-test.sh`), no custom grep-as-proof semantics.
