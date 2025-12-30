---
title: TASK-0144 Perf v1b: frame pacing hooks (windowd/renderer/ui/DSL/webview) + Perf HUD + nx perf CLI + markers
status: Draft
owner: @ui
created: 2025-12-25
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - perfd tracer: tasks/TASK-0143-perf-v1a-perfd-frame-trace-metrics.md
  - SystemUI→DSL baseline: tasks/TASK-0120-systemui-dsl-migration-phase1b-os-wiring-postflight.md
  - Renderer baseline: tasks/TASK-0054-ui-v1a-cpu-renderer-host-snapshots.md
  - windowd compositor baseline: tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md
  - webviewd baseline: tasks/TASK-0111-ui-v19a-webviewd-sandbox-offscreen.md
---

## Context

Once `perfd` exists, we need:

- instrumentation points across the UI stack,
- a user-visible Perf HUD overlay,
- a CLI for starting/stopping sessions and exporting traces.

## Goal

Deliver:

1. Frame pacing hooks:
   - `windowd`: vsync/compose/present spans; input dispatch latency span
   - renderer (CPU path): scene build/raster/blit spans
   - `ui/kit`: layout/measure/style/text spans (coarse in v1)
   - DSL runtime: load/diff/reconcile/paint spans (interp and AOT)
   - `webviewd`: coarse layout/raster spans (v1)
   - per-frame: send `frameTick(now_ns, cpu_ms, ui_ms, render_ms, present_ok)`
2. Perf HUD overlay:
   - toggle via Quick Settings tile and prefs key (`ui.perf_hud=true`)
   - shows fps + avg/p95 + last N frame bars and budget line
   - can start/stop recording and export last trace (URI toast)
   - markers:
     - `perf: hud on`
     - `perf: record on/off`
3. `nx perf` CLI:
   - start/stop/export-last/show/compare (host-first functionality; OS gating for control)
   - stable output lines for deterministic parsing

## Non-Goals

- Kernel changes.
- Perfect timing accuracy; v1 aims for consistent phase structure, not nanosecond truth.
- Mandatory dependency on `metricsd`/`logd` (perfd is enough for v1 HUD/gates).

## Constraints / invariants (hard requirements)

- Instrumentation must be low overhead and bounded; throttle UART markers.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- unit tests for instrumentation wrappers (begin/end pairing correctness)
- HUD rendering snapshot tests (optional; can be included in perf gates task)

### Proof (OS/QEMU) — gated

- `perf: hud on` marker appears when toggled
- trace export marker appears when recording is stopped

## Touched paths (allowlist)

- `source/services/perfd/` (client usage)
- `source/services/windowd/` and UI stack crates (instrumentation)
- `tools/nx-perf/` (new)
- SystemUI overlays (perf_hud)
- `docs/perf/` (follow-up task or here)

## Plan (small PRs)

1. Add perfd client + instrumentation wrappers (no markers spam)
2. Add Perf HUD overlay + QS tile + markers
3. Add nx-perf CLI

