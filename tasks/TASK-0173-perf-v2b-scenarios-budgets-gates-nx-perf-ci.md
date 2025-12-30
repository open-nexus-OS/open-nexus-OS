---
title: TASK-0173 Perf v2b (host+OS): deterministic scenarios + budgets + gates + nx-perf CLI + CI reports/docs (QEMU-safe)
status: Draft
owner: @reliability
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - perfd v2 core: tasks/TASK-0172-perf-v2a-perfd-sessions-stats-export.md
  - Perf v1 instrumentation plan (optional reuse): tasks/TASK-0144-perf-v1b-instrumentation-hud-nx-perf.md
  - IME v2 OS path (scenario deps): tasks/TASK-0147-ime-text-v2-part1b-osk-focus-a11y-os-proofs.md
  - Search v2 UI OS path (scenario deps): tasks/TASK-0152-search-v2-ui-os-deeplinks-selftests-postflight-docs.md
  - Media UX v1 OS path (scenario deps): tasks/TASK-0156-media-ux-v1b-os-miniplayer-lockscreen-sample-cli-selftests.md
  - DSoftBus v1 localSim OS path (scenario deps): tasks/TASK-0158-dsoftbus-v1b-os-consent-policy-registry-share-demo-cli-selftests.md
  - Renderer/windowd present markers (scenario deps): tasks/TASK-0170-renderer-abstraction-v1b-os-windowd-wiring-textshape-perf-markers.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Perf gates become flaky if they depend on wall-clock timing. v2 uses:

- deterministic **scenario drivers** inside the OS image (synthetic input scripts),
- deterministic **budget evaluation** in perfd,
- host-controlled QEMU runs that parse stable markers and collect `.nptr` artifacts.

Key rule: host CLIs (`nx-perf`) must not run inside QEMU selftests. QEMU selftests should only run OS apps/services.

## Goal

Deliver:

1. Session catalog + budgets config:
   - define a config file (JSON) that lists sessions, warmup/sample sizes, and gate thresholds
   - keep it host-first and not dependent on `configd` being present
2. Deterministic scenario runner (OS app):
   - `source/apps/perf-scenarios` runs:
     - windowd_frame (present loop)
     - search_palette (type-ahead + results render)
     - ime_path (preedit/commit loop incl. OSK)
     - media_panel (mini-player controls loop)
     - dsoftbus_share (128KiB msg/byte transfer progress loop)
   - each scenario:
     - begins the perfd session
     - drives a fixed event script (no RNG jitter)
     - ends and prints a stable gate marker:
       - `PERF: <session> mean=... p95=... long=... gate=ok|fail`
3. `nx perf` CLI (host):
   - `nx perf gate --all`:
     - launches QEMU, runs `perf-scenarios`, collects `.nptr` exports, and fails on any gate=fail
   - `nx perf report`:
     - reads `.nptr` and prints stable summaries (and `--json`)
   - `nx perf list` prints sessions and budgets
4. CI wiring:
   - `ci/perf_gates.sh` runs host-only:
     - QEMU run(s) bounded per scenario
     - stores artifacts under `artifacts/perf/`
     - prints concise failure summary
5. OS selftests (bounded, small):
   - in `selftest-client`, run reduced scenario sizes and assert markers:
     - `SELFTEST: perf v2 windowd ok`
     - `SELFTEST: perf v2 search ok`
     - `SELFTEST: perf v2 ime ok`
     - `SELFTEST: perf v2 media ok`
     - `SELFTEST: perf v2 bus ok`
6. Docs:
   - sessions/scenarios/gates policy
   - how to reproduce failures locally
   - how to adjust budgets responsibly

## Non-Goals

- Kernel changes.
- Hard “timing-based CI” in QEMU if not stable; if instability is found, gates must be host-first on deterministic frame streams (fallback).

## Constraints / invariants (hard requirements)

- Deterministic input scripts; no randomness or time jitter dependencies.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake success: `PERF: ... gate=ok` only if the gate logic truly passed.

## Red flags / decision points (track explicitly)

- **YELLOW (scenario dependencies)**:
  - some scenarios depend on other tasks being real (IME/Search/Media/DSoftBus/renderer).
  - until a scenario dependency is present, it must be explicitly skipped with a `stub/placeholder` marker and must not claim “ok”.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `nx perf gate --all` completes deterministically (within bounded timeouts) on supported CI environments.
  - artifacts are produced deterministically (stable naming rules).

- **Proof (OS/QEMU)**:
  - `scripts/qemu-test.sh` expected markers include the five `SELFTEST: perf v2 ... ok` lines (mini scenarios).

## Touched paths (allowlist)

- `source/apps/perf-scenarios/` (new)
- `tools/nx-perf/` (new or extend existing `nx perf` from v1 plan)
- `ci/perf_gates.sh` (new)
- `tests/perf_v2_host/` (reuse from v2a)
- `source/apps/selftest-client/`
- `docs/perf/` (new/extend)
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. Define budgets config + per-session gate evaluation rules
2. Implement perf-scenarios app (deterministic scripts + markers)
3. Implement nx-perf host CLI and CI gate script + artifact export
4. Add mini scenarios to selftest-client + docs

## Acceptance criteria (behavioral)

- CI can run deterministic perf scenarios and fail with stable summaries when budgets are exceeded.

