---
title: TASK-0055B UI v1c: visible QEMU scanout bootstrap (simplefb window + first visible frame)
status: Draft
owner: @ui @runtime
created: 2026-03-28
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - UI v1b headless compositor baseline: tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md
  - Renderer abstraction OS wiring: tasks/TASK-0170-renderer-abstraction-v1b-os-windowd-wiring-textshape-perf-markers.md
  - Display host core: tasks/TASK-0250-display-v1_0a-host-simplefb-compositor-backend-deterministic.md
  - Display OS integration follow-up: tasks/TASK-0251-display-v1_0b-os-fbdevd-windowd-integration-cursor-selftests.md
  - Device MMIO access model: tasks/TASK-0010-device-mmio-access-model.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

`TASK-0055` gives us a headless `windowd` present path with deterministic markers, but no visible display.
That is sufficient for early CI bring-up, yet it is too abstract for UI and app iteration.

We need the earliest possible **real guest-visible scanout** in QEMU so that later Launcher/SystemUI/DSL work can
be seen in an actual window, without waiting for the full Display v1.0 task family.

This task is intentionally a **bootstrap slice**:

- one fixed visible display path,
- one fixed resolution and pixel format,
- one deterministic QEMU graphics window,
- and no second compositor or temporary host-side mirror path.

## Goal

Deliver:

1. Minimal visible scanout path for QEMU `virt`:
   - replace pure `-nographic` bring-up for the UI path with a deterministic graphics-capable QEMU mode
   - expose one linear framebuffer/surface for bootstrap use
   - document the exact guest-visible resolution, stride rules, and pixel format
2. Bootstrap display authority:
   - use the same authority name that later survives into Display v1 (`fbdevd` if introduced here)
   - if the full service is not ready, use a clearly labeled bootstrap mode rather than inventing a parallel service
3. Proof of visible output:
   - a deterministic test pattern or splash frame appears in the QEMU graphics window
   - UART markers remain available for CI and bounded selftests
4. Clear handoff boundary:
   - this task unlocks visible OS bring-up for `windowd`, SystemUI, and DSL
   - richer display features (cursor, dirty rects, settings, CLI) remain for follow-up tasks

## Non-Goals

- Full Display v1.0 (`TASK-0250`/`TASK-0251`).
- Cursor support.
- Input routing.
- Multi-display or hotplug.
- GPU acceleration or virtio-gpu.

## Constraints / invariants (hard requirements)

- No second renderer or second display stack.
- The visible scanout path must sit behind the same `windowd`/renderer contracts that later tasks use.
- Deterministic guest-visible output for a fixed build and fixed boot path.
- No fake success: visible-frame markers only after a real frame is written to the visible buffer.
- Keep the bootstrap surface small and fixed; avoid feature creep.

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — required

UART markers:

- `display: bootstrap on`
- `display: mode 1280x800 argb8888`
- `display: first scanout ok`
- `SELFTEST: display bootstrap visible ok`

Visual proof:

- QEMU opens a graphics window
- a deterministic bootstrap pattern or splash frame is visible without manual guest interaction

## Touched paths (allowlist)

- QEMU runner/harness configuration for graphics-capable UI boot
- display bootstrap service or `fbdevd` bootstrap mode
- `source/services/windowd/` (only as needed to target the visible buffer)
- `source/apps/selftest-client/`
- `docs/display/simplefb_v1_0.md` or an earlier bootstrap display doc
- `docs/dev/ui/testing.md`

## Plan (small PRs)

1. QEMU graphics-capable boot mode + deterministic harness plumbing
2. bootstrap scanout authority + visible test pattern marker
3. docs + selftests + handoff notes to `TASK-0055C` and `TASK-0251`
