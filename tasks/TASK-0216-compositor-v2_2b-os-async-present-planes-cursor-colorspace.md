---
title: TASK-0216 Compositor v2.2b (OS/QEMU): gpuabst integration + async present queue + plane planner (primary/overlay/cursor) + cursor plane + color space plumbing + metrics/CLI + selftests/docs
status: Draft
owner: @ui
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Compositor v2.2 host substrate: tasks/TASK-0215-compositor-v2_2a-host-gpuabst-planner-async-color.md
  - Windowing v2.1 OS surfaces/fences baseline: tasks/TASK-0208-windowing-v2_1b-os-swapchains-fences-vsync-domains-hidpi.md
  - Windowing/Compositor v2 OS integration (WM-lite, screencap): tasks/TASK-0200-windowing-compositor-v2b-os-wm-lite-alt-tab-screencapd.md
  - Persistence substrate (/state for metrics export): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Policy cap matrix baseline: tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Compositor v2.2a defines gpuabst + plane planning + async present and color space plumbing host-first.
This task wires those concepts into OS/QEMU `windowd` while staying CPU-rendered:

- gpuabst CPU backend drives composition ops,
- async present queue decouples compose from present (bounded, drop-oldest),
- plane planner prepares primary/overlay/cursor assignments,
- cursor plane is represented as a small dedicated surface/image,
- color space stubs (sRGB/Linear) are plumbed through surface→compose→present,
- metrics are exposed deterministically via overlay/CLI; `/state` persistence is gated.

## Goal

Deliver:

1. gpuabst integration:
   - windowd uses `gpuabst::Device/Queue` abstraction for compose ops (CPU backend)
   - markers:
     - `gpuabst: device cpu`
     - `gpuabst: submit ops=<n>`
2. Plane planner in windowd:
   - per-frame `Plan` selection with deterministic heuristics
   - markers:
     - `planner: plan primary=<n> overlays=<n> cursor=<0|1>`
     - `planner: overlay promote win=<id>` (when used)
3. Async present queue:
   - bounded queue size from schema (default 3)
   - drop-oldest on overflow; deterministic counter increment
   - markers:
     - `presentq: enqueue seq=<n> len=<k>`
     - `presentq: drop oldest seq=<n>`
4. Cursor plane (prep):
   - cursor bitmap set/move/hide APIs inside windowd (or a small cursor service later)
   - cursor plane composited last
   - markers:
     - `cursor: set w=<w> h=<h>`
     - `cursor: move x=<x> y=<y>`
5. Color space plumbing:
   - surfaces carry `ColorSpace` (default sRGB)
   - display has an output colorspace (default sRGB)
   - CPU conversion uses deterministic LUT; explicit rounding policy
   - markers:
     - `display: colorspace=<Srgb|Linear>`
6. Metrics + overlay + CLI:
   - extend frame timings overlay to show:
     - queue_len, dropped_frames, overlay_promotions, cursor_plane_used, colorspace mode
   - `nx-win` extensions:
     - `planes`, `async on|off`, `colorspace`, `metrics`
   - `/state` export of compositor metrics (JSONL) is gated on `TASK-0009`; without `/state`, export is disabled and must not claim persistence.
7. OS selftests (bounded):
   - `SELFTEST: comp overlay promote ok`
   - `SELFTEST: comp cursor plane ok`
   - `SELFTEST: comp async drop ok`
   - `SELFTEST: comp colorspace ok`

## Non-Goals

- Kernel changes.
- Real GPU planes/hardware scanout.
- Full color management (HDR, ICC profiles).

## Constraints / invariants (hard requirements)

- No fake success:
  - overlay promotion proof must be based on planner state, not log greps.
  - async drop proof must validate `dropped_frames` counter changes deterministically.
- Deterministic bounds and timeouts; no busy-wait.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p compositor_v2_2_host -- --nocapture` (from v2.2a)

- **Proof (QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=195s ./scripts/qemu-test.sh`
  - Required markers:
    - `SELFTEST: comp overlay promote ok`
    - `SELFTEST: comp cursor plane ok`
    - `SELFTEST: comp async drop ok`
    - `SELFTEST: comp colorspace ok`

## Touched paths (allowlist)

- `source/services/windowd/` (planner + async present + cursor + colorspace)
- `userspace/libs/gpuabst/` (OS integration)
- SystemUI overlays (timings)
- `tools/nx-win/` (extend)
- `schemas/compositor_v2_2.schema.json`
- `source/apps/selftest-client/`
- `docs/compositor/` + `docs/tools/nx-win.md` + `docs/ui/testing.md`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. integrate gpuabst cpu backend into compose path
2. plane planner + cursor plane prep + markers
3. async present queue + metrics counters + markers
4. colorspace plumbing + LUT conversions
5. overlay/CLI + selftests + docs + postflight wrapper (delegating)

## Acceptance criteria (behavioral)

- In QEMU, plane planning, async present queue behavior, cursor plane markers, and colorspace toggles are proven deterministically via selftest markers.

