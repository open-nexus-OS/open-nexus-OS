---
title: TASK-0205 WebView v1.2a (host-first): persistent history model + session crash-recovery model + CSP report format/export + cookie jar v0 (dev) + deterministic tests
status: Draft
owner: @ui
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Ads Safety + Family Mode (track): tasks/TRACK-ADS-SAFETY-FAMILYMODE.md
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
     - default backend: file-based **Cap'n Proto snapshot** (canonical, `.nxs`) with deterministic encoding
       - derived/debug exports may use NDJSON/JSONL (deterministic)
     - optional libSQL backend (feature-gated); must not be required for OS viability unless explicitly chosen later
2. Session restore ("crash recovery") model:
   - stable `session.nxs` schema (Cap'n Proto snapshot; canonical):
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
5. **Servo experimental backend (opt-in, feature-gated)**:
   - `#[cfg(feature = "experimental-servo")]` integration as a WebView backend
   - **host-first**: Servo embedding runs headless on host for initial smoke tests
   - **deterministic fallback**: simple renderer remains default; Servo is additive
   - **bounded**: cap Servo process memory/threads via policy (host fixtures)
   - **crash-safe**: Servo runs in separate process; crashes must not panic host harness
   - markers (host-only in v1.2a):
     - `servo: backend init (experimental)`
     - `servo: page loaded url=<...> (best-effort)`
     - `servo: crashed (expected)` (when testing crash isolation)
6. Host tests `tests/webview_v1_2_host/`:
   - history record/search/export determinism (no wallclock)
   - session save/restore determinism
   - CSP report append/filter/export determinism
   - cookie jar enabled: set + attach; disabled: attaches none
   - **Servo experimental (when feature enabled)**:
     - smoke test: Servo backend initializes and loads a simple page (best-effort, may be flaky)
     - crash isolation: simulated Servo crash does not panic host harness

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

- **GREEN → YELLOW (Servo experimental integration strategy)**:
  - **Context**: The "simple text+CSS renderer" approach is too conservative for 2026. Modern websites (React/Vue/GitHub/YouTube)
    require a real browser engine. Without it, the OS appears as a "toy OS" and limits adoption.
  - **Decision**: Add **Servo experimental** as an opt-in parallel track alongside the simple renderer:
    - **Simple renderer** (default, always built): deterministic, bounded, for Settings/Help/Docs.
    - **Servo experimental** (opt-in, `EXPERIMENTAL_SERVO=1`): real browser engine, for modern websites.
  - **Rationale**:
    - ✅ **User expectation**: "Browser" in 2026 means "opens real websites", not "plain HTML viewer".
    - ✅ **Härtetest for OS isolation**: Servo crashes frequently → proves that app crashes don't crash SystemUI/kernel.
    - ✅ **Stress test for `logd` + crash reporting**: Servo's complex crashes (multi-thread, GPU, deep stacks) will battle-test
      the OS's structured crash reporting and prove it's better than Linux/macOS coredumps.
    - ✅ **Community benefit**: First OS with Servo as system WebView → visibility + production crash data for Servo project.
    - 🟡 **Build complexity**: Servo is large; mitigated by making it opt-in (doesn't slow default CI).
    - 🟡 **Determinism**: Servo goldens will be flaky; mitigated by marking them "best-effort, not gated".
  - **Implementation posture (v1.2)**:
    - Host-first: Servo integration starts in `TASK-0205` as a **feature-gated backend** (`#[cfg(feature = "experimental-servo")]`).
    - Deterministic fallback: Simple renderer remains the default and deterministic baseline.
    - Opt-in build: `EXPERIMENTAL_SERVO=1 make build` or `just build-servo` (not in default CI).
    - Separate app: `source/apps/browser_servo/` (parallel to `source/apps/browser/`).
  - **Follow-up (v1.2b OS wiring, `TASK-0206`)**:
    - Servo browser app in SystemUI launcher (with ⚠️ "Experimental" badge).
    - Crash isolation proofs (Servo crash → SystemUI stable).
    - Structured crash reports (stack trace + URL + last action + policy context).
    - Opt-in telemetry to Servo project (with user consent).
  - **Red flag decision point (pre-release)**: Before first public release, assess Servo stability:
    - **Option 1**: Servo stable enough → ship as "experimental, use at own risk".
    - **Option 2**: Servo too crashy → developer-only build, not shipped.
    - **Option 3**: Servo production-ready → becomes default browser (simple renderer becomes fallback).

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p webview_v1_2_host -- --nocapture`

## Touched paths (allowlist)

- `userspace/libs/webhistory/` (new)
- `userspace/libs/cookiejar/` (new; host-only)
- `userspace/libs/webview-core/` (extend session restore hooks)
- `userspace/libs/webview-servo/` (new; `#[cfg(feature = "experimental-servo")]` only)
- `tests/webview_v1_2_host/` (new)
- `docs/webview/` (minimal doc here or in v1.2b)
- `docs/webview/servo-experimental.md` (new; Servo integration strategy + build instructions)

## Plan (small PRs)

1. history model + deterministic search/export + tests
2. session restore model + tests
3. CSP report schema/export + tests
4. cookie jar v0 (dev toggle) + tests
5. Servo experimental backend (opt-in, feature-gated) + host smoke tests + crash isolation test

## Acceptance criteria (behavioral)

- Host tests deterministically prove history/session/CSP export/cookie dev toggle behavior.
