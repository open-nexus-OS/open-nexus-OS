---
title: TASK-0145 Perf v1c (host-first): deterministic perf gates for key scenes + baseline artifacts + OS markers (gated)
status: Draft
owner: @reliability
created: 2025-12-25
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - perfd tracer: tasks/TASK-0143-perf-v1a-perfd-frame-trace-metrics.md
  - Perf instrumentation/HUD: tasks/TASK-0144-perf-v1b-instrumentation-hud-nx-perf.md
  - UI performance philosophy: docs/dev/ui/foundations/quality/performance-philosophy.md
  - SystemUI DSL pages (scenes): tasks/TASK-0119-systemui-dsl-migration-phase1a-launcher-qs-host.md
  - Notifications center DSL: tasks/TASK-0121-systemui-dsl-migration-phase2a-settings-notifs-host.md
---

## Context

“Perf gates” can become flaky if they depend on wall-clock timing under QEMU. For v1 we define:

- deterministic synthetic workloads,
- deterministic frame event streams,
- and gates that are validated host-first.

OS/QEMU gates are optional and must be explicitly labeled as “best-effort” unless we can make them stable.

Scope note:

- Perf v2 refresh (sessions/budgets/scenarios/reports) is tracked as `TASK-0172`/`TASK-0173`.
  v2 should replace this v1 gates approach rather than adding a second, competing gate runner.

## Goal

Deliver:

1. Deterministic perf gate harness (host-first):
   - runs named sessions (Launcher, Quick Settings, Notifications Center, Settings→Display)
   - drives synthetic input deterministically (fixed event script)
   - collects metrics from `perfd` and checks thresholds
   - stores trace exports as artifacts for debugging
2. Gate thresholds:
   - stored as a config/recipe file (do not depend on `configd` being present)
   - stable fail markers and stable reason output
   - support threshold groups for:
     - frame timing,
     - hot-path budgets (wakeups / queue residence / service hops / cross-core hops),
     - UI runtime budgets (recompute fanout / observer count / wasted recomputes),
     - and zero-copy honesty (copy fallback / control-plane bytes where the scene uses bulk payloads)
3. Markers:
   - `perf: gate pass <name> avg=... p95=... long=...`
   - `perf: gate fail <name> ...`
4. Optional OS selftest gating (only if it is stable):
   - `SELFTEST: perf gate <scene> ok`

## Non-Goals

- Kernel changes.
- Making QEMU timing-based gates a hard requirement if they cannot be stabilized.

## Constraints / invariants (hard requirements)

- Deterministic inputs and deterministic metric calculations.
- Gates should prefer architecture-health budgets over fragile micro-timing when the latter is not stable in QEMU.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/perf_gates_host/`:

- each scene gate passes against a deterministic baseline
- failure case produces a stable diagnostic summary
- trace exports are bounded and deterministically named (no timestamps in filenames unless fixed)

### Proof (OS/QEMU) — optional/gated

- only add OS markers if the scene runner is stable in QEMU

## Touched paths (allowlist)

- `tests/perf_gates_host/` (new)
- `docs/perf/gates.md` (new)
- `tools/postflight-perf-v1.sh` (delegating wrapper, optional later)

## Plan (small PRs)

1. Implement host gate runner + fixture scenes + baseline thresholds
2. Add artifact export and docs
3. Add OS markers only if stable

## Phase plan

### Phase A — Timing gates

- keep the original frame-timing scenes and thresholds.

### Phase B — Architecture-health gates

- add deterministic thresholds for wakeups, queue residence, recompute fanout, observer count, and zero-copy fallback
  behavior on the same named scenes.
