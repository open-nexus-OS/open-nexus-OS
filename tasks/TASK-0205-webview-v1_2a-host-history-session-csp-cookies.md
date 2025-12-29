---
title: TASK-0205 WebView v1.2a (host-first): persistent history model + session crash-recovery model + CSP report format/export + cookie jar v0 (dev) + deterministic tests
status: Draft
owner: @ui
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - WebView v1.1 core (history/find/storage/CSP strict): tasks/TASK-0186-webview-v1_1a-host-webview-core-history-find-sessionstorage-csp.md
  - WebView Net v1 OS services (fetchd/downloadd fixtures-only): tasks/TASK-0177-webview-net-v1b-os-httpstubd-fetchd-downloadd-policy-selftests.md
  - Devnet TLS fetch path (real HTTP(S), host-first): tasks/TASK-0193-networking-v1a-host-devnet-tls-fetchd-integration.md
  - Persistence substrate (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
---

## Context

WebView v1.1 delivers an offline, deterministic WebView core with in-memory history, session storage, find-in-page,
and CSP-strict enforcement.

WebView v1.2 adds:

- persistent history across runs,
- crash recovery/session restore for tabs,
- a persistent CSP report log plus a viewer/export surface (OS wiring in v1.2b),
- and a minimal cookie jar v0 as a **dev-only host toggle** for same-origin session cookies.

This task is host-first and defines the data formats, determinism rules, and tests.

## Goal

Deliver:

1. `userspace/libs/webhistory` (storage abstraction + deterministic semantics):
   - record visits, update scroll, recent/search, export NDJSON, clear
   - deterministic ordering and search folding rules (explicit)
   - injected clock interface for tests (no wallclock dependency)
   - storage backends:
     - default host test backend: file-based (JSONL or small binary) with deterministic encoding
     - optional libSQL backend (feature-gated); must not be required for OS viability unless explicitly chosen later
2. Session restore (“crash recovery”) model:
   - stable `session.json` schema:
     - tabs: `{url, scroll_y}` and active index
     - bounded max tabs
   - deterministic write cadence rules:
     - on navigation commit and at most every N seconds (timer injected in tests)
3. CSP report log format:
   - stable JSONL schema for CSP violations:
     - `ts_seq` (monotonic sequence), `doc_url`, `directive`, `blocked_url`, `disposition`
   - deterministic export NDJSON rules (stable ordering)
   - NOTE: OS path writes under `state:/csp/reports.jsonl` (v1.2b; `/state` gated)
4. Cookie jar v0 (host-only dev toggle):
   - same-origin, session cookies only
   - deterministic attach order
   - disabled by default
   - no persistence by default; optional persistence is v1.2b and must be dev-only and `/state` gated
5. Host tests `tests/webview_v1_2_host/`:
   - history record/search/export determinism (no wallclock)
   - session save/restore determinism
   - CSP report append/filter/export determinism
   - cookie jar enabled: set + attach; disabled: attaches none

## Non-Goals

- Kernel changes.
- OS/QEMU integration (downloads, CSP viewer UI, persistent files) lives in v1.2b.
- Real networking by default; devnet remains gated.

## Constraints / invariants (hard requirements)

- Determinism: injected clocks; stable ordering; stable encoding.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **YELLOW (libSQL vs pure-Rust)**:
  - The prompt requests libSQL for history. We can support libSQL as an optional backend, but we must keep a deterministic
    fallback backend and avoid making OS viability depend on libSQL unless explicitly decided.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p webview_v1_2_host -- --nocapture`

## Touched paths (allowlist)

- `userspace/libs/webhistory/` (new)
- `userspace/libs/cookiejar/` (new; host-only)
- `userspace/libs/webview-core/` (extend session restore hooks)
- `tests/webview_v1_2_host/` (new)
- `docs/webview/` (minimal doc here or in v1.2b)

## Plan (small PRs)

1. history model + deterministic search/export + tests
2. session restore model + tests
3. CSP report schema/export + tests
4. cookie jar v0 (dev toggle) + tests

## Acceptance criteria (behavioral)

- Host tests deterministically prove history/session/CSP export/cookie dev toggle behavior.
