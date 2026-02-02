---
title: TASK-0206 WebView v1.2b (OS/QEMU): persistent history + session restore + downloadd pause/resume (devnet-gated) + content:// leases + CSP report viewer + selftests/docs
status: Draft
owner: @ui
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Ads Safety + Family Mode (track): tasks/TRACK-ADS-SAFETY-FAMILYMODE.md
  - WebView v1.2 host substrate: tasks/TASK-0205-webview-v1_2a-host-history-session-csp-cookies.md
  - WebView v1.1 OS wiring (file chooser/leases): tasks/TASK-0187-webview-v1_1b-os-file-chooser-content-leases-nxweb-selftests.md
  - WebView Net v1 OS services (httpstubd/fetchd/downloadd): tasks/TASK-0177-webview-net-v1b-os-httpstubd-fetchd-downloadd-policy-selftests.md
  - Devnet real HTTP(S) (OS-gated): tasks/TASK-0194-networking-v1b-os-devnet-gated-real-connect.md
  - Persistence substrate (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Policy cap matrix baseline: tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Testing contract: scripts/qemu-test.sh
  - Data formats rubric (JSON vs Cap'n Proto): docs/adr/0021-structured-data-formats-json-vs-capnp.md
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
   - persist `state:/web/session.nxs` (Cap'n Proto snapshot; canonical) bounded
     - optional derived/debug view: `nx web session export --json` emits deterministic JSON
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
6. **Servo experimental browser app (opt-in, parallel to simple browser)**:
   - new app: `source/apps/browser_servo/` (only built when `EXPERIMENTAL_SERVO=1` or via `just build-servo`)
   - SystemUI launcher entry: "Browser (Servo, Experimental)" with ⚠️ badge
   - first-launch warning: "This browser is experimental and may crash. Use at your own risk."
   - **Crash isolation proofs (critical)**:
     - Servo crash → SystemUI remains responsive (no freeze/hang)
     - Servo crash → other apps continue running (no system-wide impact)
     - Servo crash → kernel/samgr/windowd remain stable (no kernel panic)
   - **Structured crash reporting integration**:
     - on Servo crash, `logd` generates structured crash report:
       - symbolized stack trace (all threads)
       - last URL loaded
       - last user action (input event from windowd)
       - process state (memory usage, thread count, capability set)
       - policy context (which caps were active, which were denied)
     - crash report stored under `state:/crashes/servo/<timestamp>.nxs` (Cap'n Proto snapshot; canonical)
       - derived/debug view: `nx crash export <id> --json` emits deterministic JSON
     - optional: `nx crash upload <id>` sends report to Servo project (opt-in, user consent required)
   - markers:
     - `browser_servo: launched (experimental)`
     - `browser_servo: page loaded url=<...> (best-effort)` (may be flaky; not gated)
     - `browser_servo: crashed` (expected during testing)
     - `logd: crash report generated id=<...>`
     - `SELFTEST: browser_servo isolation ok` (SystemUI sends SIGKILL to Servo, verifies system stability)
     - `SELFTEST: browser_servo crash report ok` (verifies crash report is complete and parsable)
7. `nx-web` extensions:
   - history recent/search/export/clear
   - download list/pause/resume/open/clear
   - NOTE: QEMU selftests must not rely on running host tools inside QEMU
8. `nx crash` CLI (new, for Servo crash debugging):
   - `nx crash list` — list recent crashes (Servo + other apps)
   - `nx crash show <id>` — display structured crash report (human-readable)
   - `nx crash export <id> --json` — export crash report as deterministic JSON
   - `nx crash upload <id>` — upload to Servo project (opt-in, requires user consent + network)
9. OS selftests (bounded, deterministic):
   - session restore:
     - `SELFTEST: web session restore ok`
   - downloads:
     - fixture download ok (from v1): `SELFTEST: web download ok` (already in `TASK-0177`)
     - devnet resume path (only if devnet enabled/unblocked): `SELFTEST: web download resume ok`
   - export:
     - `SELFTEST: web export ok` (history + CSP exports exist; gated on `/state`)
   - clear history:
     - `SELFTEST: web history clear ok`
   - **Servo experimental (only when built with `EXPERIMENTAL_SERVO=1`)**:
     - `SELFTEST: browser_servo isolation ok` (crash isolation proof)
     - `SELFTEST: browser_servo crash report ok` (structured crash report proof)

## Non-Goals

- Kernel changes.
- Online/external network access by default.
- Full browser UX (tabs UI etc.) beyond the minimal restore prompt.

## Constraints / invariants (hard requirements)

- `/state` gating: without `TASK-0009`, persistence/export must be disabled or explicitly `stub/placeholder`.
- devnet gating: Range/resume must not claim success unless real backend is enabled (`TASK-0194`).
- No fake success: selftests must validate outcomes by reading service state / file existence, not log greps.
- **Servo crash isolation (critical)**: Servo crashes must NOT crash SystemUI, kernel, or other apps. This is a hard microkernel
  isolation proof. If Servo crash causes system instability, that is a blocker bug.
- **Crash report completeness**: Structured crash reports must include all required fields (stack trace, URL, action, state, policy).
  Incomplete reports are a quality bug (not a blocker, but must be tracked).
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
  - **Servo experimental (only when built with `EXPERIMENTAL_SERVO=1`)**:
    - `browser_servo: launched (experimental)`
    - `browser_servo: page loaded url=<...> (best-effort)` (may be flaky; not a hard gate)
    - `browser_servo: crashed` (expected during isolation test)
    - `logd: crash report generated id=<...>`
    - `SELFTEST: browser_servo isolation ok` (SystemUI responsive after Servo crash)
    - `SELFTEST: browser_servo crash report ok` (crash report complete and parsable)

## Touched paths (allowlist)

- `userspace/libs/webhistory/`
- `userspace/ui/controls/webview/` + `userspace/libs/webview-core/`
- `source/services/downloadd/` (resume + leases)
- `source/services/fetchd/` (backend selection already tracked; consumption here)
- `source/services/logd/` (extend: structured crash report generation for Servo)
- `source/apps/browser_servo/` (new; `#[cfg(feature = "experimental-servo")]` only)
- SystemUI Settings pages (CSP viewer) + downloads UI (optional)
- SystemUI launcher (add "Browser (Servo, Experimental)" entry with ⚠️ badge)
- `tools/nx-web/`
- `tools/nx-crash/` (new; crash report CLI)
- `source/apps/selftest-client/` (extend: Servo isolation + crash report tests)
- `schemas/webview_v1_2.schema.json`
- `schemas/crash_report_v1.schema.json` (new; structured crash report format)
- `docs/webview/` + `docs/downloads/` + `docs/tools/nx-web.md`
- `docs/webview/servo-experimental.md` (extend: OS integration + crash reporting)
- `docs/tools/nx-crash.md` (new; crash report CLI + Servo debugging)
- `scripts/qemu-test.sh` (extend: Servo experimental markers)

## Plan (small PRs)

1. history persistence wiring + selftests
2. session restore wiring + selftests
3. CSP report log + viewer + export/clear + selftests (gated)
4. downloadd resume semantics (devnet-gated) + content leases + selftests
5. Servo experimental browser app + SystemUI launcher entry + first-launch warning
6. Servo crash isolation proofs (SIGKILL test + SystemUI responsiveness check)
7. Structured crash reporting integration (logd extension + crash report schema + nx crash CLI)
8. Servo crash report selftests (completeness + parsability + optional upload flow)
9. nx-web extensions + docs + marker contract update

## Red flags / decision points (track explicitly)

- **RED FLAG (pre-release Servo stability assessment)**:
  - **Context**: Servo experimental is built and integrated in v1.2, but shipping it in the first public release requires a stability assessment.
  - **Decision point**: Before first public release, assess Servo's crash rate, memory leaks, and security posture:
    - **Option 1 (ship experimental)**: If Servo crash rate is acceptable (e.g., <10% of page loads crash, no kernel panics, no data loss),
      ship it as "Browser (Servo, Experimental)" with clear warnings. This gives users a real browser and proves OS isolation.
    - **Option 2 (developer-only)**: If Servo is too unstable (frequent kernel panics, data corruption, unacceptable crash rate),
      keep it as a developer-only build (`EXPERIMENTAL_SERVO=1`) and do not include it in the default OS image.
    - **Option 3 (promote to default)**: If Servo becomes production-ready (stable, secure, passes all tests), promote it to the
      default browser and demote the simple renderer to a fallback for lightweight use cases.
  - **Metrics to track (for decision)**:
    - Crash rate (% of page loads that crash Servo)
    - Kernel panic rate (must be 0%)
    - SystemUI freeze rate after Servo crash (must be 0%)
    - Memory leak severity (acceptable if bounded and recoverable)
    - Security audit status (CVE count, exploit feasibility)
  - **Timeline**: Assess 2-4 weeks before first public release; revisit every release cycle.

## Acceptance criteria (behavioral)

- In QEMU, WebView history/session persistence and CSP report viewing/export behave deterministically when `/state` exists.
- Download resume is only claimed when devnet is enabled and Range support is proven; otherwise behavior is explicit and deterministic.
- **Servo experimental (when built)**: Servo crashes do not crash SystemUI, kernel, or other apps. Structured crash reports are
  complete and parsable. This proves microkernel isolation and crash reporting quality.
