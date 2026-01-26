---
title: TASK-0056 UI v2a: double-buffered surfaces + present scheduler (vsync/fences/latency) + input routing (hit-test/focus)
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - UI v1a renderer (baseline): tasks/TASK-0054-ui-v1a-cpu-renderer-host-snapshots.md
  - UI v1b windowd (baseline): tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md
  - Drivers/Accelerators contracts: tasks/TRACK-DRIVERS-ACCELERATORS.md
  - VMO plumbing: tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - QoS/timers (vsync spine): tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md
  - Config broker (ui knobs): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Policy as Code (permissions): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

UI v1 brought up a minimal compositor and surface protocol. UI v2 introduces the first “real-time UX” aspects:

- double buffering (no tearing),
- a present scheduler aligned to vsync,
- input routing with hit-testing and focus.

This task is deliberately **render-backend agnostic**:

- CPU backend is used for v2a proofs.
- A future GPU backend (Imagination/PowerVR or virtio-gpu) can plug into the same interfaces once the
  device-class service stack is available (see `TRACK-DRIVERS-ACCELERATORS`).

Scope note:

- A focused “Windowing/Compositor v2” integration slice (damage regions, input regions hit-testing, deterministic screencaps/thumbs, WM-lite + alt-tab) is tracked separately as `TASK-0199`/`TASK-0200` to avoid stretching v2a into WM/screencap territory.

## Goal

Deliver:

1. **Double-buffered surfaces**:
   - client acquires a back buffer VMO, writes into it, then presents by frame index.
2. **Present scheduler** in `windowd`:
   - vsync-aligned present,
   - coalescing of rapid submits,
   - bounded fences and deterministic markers,
   - basic latency metrics (internal counters; log markers are enough for v2a).
3. **Input routing**:
   - hit-testing through the layer tree,
   - focus model (focus follows click),
   - pointer and keyboard delivery to surfaces.
4. Host tests for present scheduling + input routing, and OS markers for QEMU.

## Non-Goals

- Kernel changes.
- Text shaping and SVG (v2b).
- Real HW vsync; v2 uses a timer-driven vsync spine.
- Low-level input device drivers (HID/touch) or input event pipeline (handled by `TASK-0252`/`TASK-0253`; this task focuses on input routing within windowd).

## Constraints / invariants (hard requirements)

- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Bounded scheduler state:
  - cap queue depth per surface,
  - cap coalesced damage rect count.
- Premultiplied alpha rules must be consistent across present/composition.
- No parallel sync model: fences must be versioned and minimal (documented as v2a semantics).

## Red flags / decision points

- **YELLOW (fence semantics)**:
  - v2a “present fence” can be a minimal event signaled after composition tick.
  - Must be documented as minimal and not yet suitable for true low-latency pipelines.
- **YELLOW (CPU-only wording)**:
  - We do not lock ourselves into CPU-only. Interfaces must not assume CPU blits.
  - But v2a proofs remain CPU-based to keep the task QEMU-tolerant.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v2a_host/`:

- present scheduler:
  - client produces N frames rapidly → scheduler coalesces and presents fewer times deterministically
  - fences are signaled after present
  - “no damage → no present”
- input routing:
  - two overlapping surfaces → pointer hit-test selects the topmost visible surface
  - focus transitions on click and keyboard delivery goes to focused surface

### Proof (OS/QEMU) — gated

UART markers (order tolerant):

- `windowd: present scheduler on`
- `windowd: input on`
- `windowd: focus -> <surface_id>`
- `launcher: click ok`
- `SELFTEST: ui v2 present ok`
- `SELFTEST: ui v2 input ok`

## Touched paths (allowlist)

- `source/services/windowd/` + `source/services/windowd/idl/` (extend)
- `userspace/apps/launcher/` (update to double-buffer and input click demo)
- `tests/ui_v2a_host/` (new)
- `source/apps/selftest-client/` (markers)
- `tools/postflight-ui-v2a.sh` (delegating)
- `docs/dev/ui/input.md` + `docs/dev/ui/renderer.md` (new/extend)

## Plan (small PRs)

1. **IDL updates**
   - Add `AcquireBackBuffer()` + `Present(frame_idx, damage, fence)`
   - Extend `input.capnp` with pointer/keyboard events

2. **Present scheduler**
   - per-surface double buffer state
   - vsync tick alignment
   - damage coalescing rules documented
   - markers:
     - `windowd: present scheduler on`
     - `windowd: present (seq=... frames=coalesced|single latency_ms=...)`

3. **Input routing**
   - hit-test on layer tree
   - focus manager + delivery channels
   - markers:
     - `windowd: input on`
     - `windowd: focus -> ...`
     - `windowd: pointer hit ...`

4. **Launcher update**
   - double-buffer API
   - click toggles a highlight rectangle → `launcher: click ok`

5. **Proof + docs**
   - host tests + OS markers + docs
