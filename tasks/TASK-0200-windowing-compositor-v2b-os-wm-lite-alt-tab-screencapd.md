---
title: TASK-0200 Windowing/Compositor v2b (OS/QEMU): layers+damage+input regions + vsync tick + WM-lite (move/resize/snap) + Alt-Tab thumbs + screencapd + selftests/docs
status: Draft
owner: @ui
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - ADR: docs/adr/0022-modern-image-formats-avif-webp.md
  - Windowing v2 host substrate: tasks/TASK-0199-windowing-compositor-v2a-host-damage-occlusion-screencap.md
  - windowd↔renderer OS wiring baseline: tasks/TASK-0170-renderer-abstraction-v1b-os-windowd-wiring-textshape-perf-markers.md
  - Present scheduler + input routing baseline: tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md
  - WM controller baseline: tasks/TASK-0064-ui-v6a-window-management-scene-transitions.md
  - WM resize/move + shortcuts baseline: tasks/TASK-0070-ui-v8b-wm-resize-move-shortcuts-settings-overlays.md
  - Screenshot/share baseline: tasks/TASK-0068-ui-v7c-screenshot-screencap-share-sheet.md
  - Perf sessions (optional): tasks/TASK-0172-perf-v2a-perfd-sessions-stats-export.md
  - Policy caps: tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

With deterministic compositor primitives proven host-first (v2a), we wire them into OS `windowd` and SystemUI:

- layers + damage regions + input regions,
- deterministic vsync tick/present latch,
- minimal WM policies (move/resize/snap/stack),
- Alt-Tab switcher using deterministic thumbnails,
- and a deterministic screencap service exporting PNG (baseline) with WebP export option (preferred for smaller artifacts).

This is QEMU-tolerant and kernel-unchanged.

## Goal

Deliver:

1. `windowd` layer+damage model:
   - surfaces/windows have:
     - `layer`, `bounds`, `input_region`, `opaque_region`, `z`
   - damage submission uses `Damage::Rect|Region|Full`
   - markers (rate-limited):
     - `windowd: damage rect=...`
     - `windowd: compose regions=<n>`
2. Deterministic vsync tick:
   - timer-driven `hz` from config (default 60)
   - present scheduler avoids full-screen repaints when no damage
   - marker:
     - `windowd: vsync seq=<n>`
3. Input routing with regions:
   - hit-testing top→bottom across layers/z
   - focus change markers:
     - `windowd: focus -> win=<id>`
4. WM-lite policies:
   - move/resize via drag (frame hit-zones)
   - snap left/right/maximize/restore
   - raise-on-focus within App layer; overlays always above
5. Alt-Tab switcher + thumbnails:
   - SystemTop overlay listing App windows
   - thumbnails derived from cached window buffers or last composed frame (deterministic downscale)
   - markers:
     - `wm: switcher open`
     - `wm: switcher select win=<id>`
6. Screencap service `screencapd` (deterministic PNG baseline; WebP export option):
   - `full`, `window`, `thumb` APIs
   - size caps and stable errors
   - markers:
     - `screencap: full bytes=<n>`
     - `screencap: thumb win=<id> bytes=<n>`
7. CLI `nx-win` (host tool; optional in OS bring-up):
   - list/move/resize/snap/screenshot/damage-stats
   - NOTE: QEMU selftests must not require running host tools inside QEMU
8. OS selftests (bounded):
   - launch a deterministic demo with 3 overlapping windows
   - move/resize/snap path produces markers:
     - `SELFTEST: wm move/resize/snap ok`
   - damage reduces redraw (measured by metrics):
     - `SELFTEST: damage tracked ok`
   - alt-tab selection works:
     - `SELFTEST: alt-tab ok`
   - screencap works:
     - `SELFTEST: screencap ok`

## Non-Goals

- Kernel changes.
- Full tiling WM / split layouts (separate tasks).
- Full compositor v4 caches/atlases.

## Constraints / invariants (hard requirements)

- Deterministic markers and bounded timeouts.
- No fake success: “damage tracked ok” requires metrics proving non-full redraw for a bounded scene.
- perfd hooks are optional and must be gated (no “perf ok” markers unless evaluated).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p windowing_v2_host -- --nocapture` (from v2a)

- **Proof (QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=195s ./scripts/qemu-test.sh`
  - Required markers:
    - `SELFTEST: wm move/resize/snap ok`
    - `SELFTEST: damage tracked ok`
    - `SELFTEST: alt-tab ok`
    - `SELFTEST: screencap ok`

## Touched paths (allowlist)

- `source/services/windowd/`
- `source/services/screencapd/` (new or extend existing plan)
- SystemUI overlays (alt-tab, dev damage overlay)
- `userspace/apps/win-demo/` (deterministic demo)
- `source/apps/selftest-client/`
- `schemas/windowing.schema.json`
- `docs/windowing/` + `docs/tools/nx-win.md`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. windowd layer/damage model + vsync tick + markers
2. input routing via regions + focus markers
3. wm-lite move/resize/snap + alt-tab overlay + thumbnails
4. screencapd deterministic PNG + selftests + docs + marker contract update
   - WebP export option (preferred) may be added without changing marker contracts; tests should validate pixels

## Acceptance criteria (behavioral)

- In QEMU, WM-lite operations, damage-aware composition, alt-tab, and deterministic screencaps are proven by markers without fake success.

Follow-up:

- Windowing/Compositor v2.1 (GPU-ready swapchain surfaces + acquire/release timeline fences + vsync domains + HiDPI v1 + timings overlay) is tracked as `TASK-0207`/`TASK-0208`.
