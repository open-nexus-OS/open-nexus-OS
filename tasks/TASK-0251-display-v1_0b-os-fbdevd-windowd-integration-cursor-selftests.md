---
title: TASK-0251 Display v1.0b (OS/QEMU): fbdevd service + windowd simplefb integration + cursor + SystemUI splash + `nx display` + selftests
status: Draft
owner: @ui
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Authority & naming registry: tasks/TRACK-AUTHORITY-NAMING.md
  - Display core (host-first): tasks/TASK-0250-display-v1_0a-host-simplefb-compositor-backend-deterministic.md
  - Renderer abstraction: tasks/TASK-0169-renderer-abstraction-v1a-host-sceneir-cpu2d-goldens.md
  - Windowd compositor: tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md
  - Windowd wiring: tasks/TASK-0170-renderer-abstraction-v1b-os-windowd-wiring-textshape-perf-markers.md
  - Device MMIO access: tasks/TASK-0010-device-mmio-access-model.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need OS/QEMU integration for Display v1.0:

- `fbdevd` service (userspace framebuffer driver),
- windowd simplefb backend integration,
- cursor support,
- SystemUI splash.

The prompt proposes `fbdevd` and windowd integration with simplefb. `TASK-0055` and `TASK-0170` already plan windowd compositor with headless present (VMO buffers). This task extends it with **real framebuffer output** via simplefb, complementing the headless path.

## Goal

On OS/QEMU:

1. **DTB & runner updates**:
   - extend `pkg://dts/virt-nexus.dts` with simple-framebuffer node: `framebuffer@40000000` (1280x800 ARGB8888, 8 MiB)
   - rebuild DTB; ensure QEMU `-machine virt -nographic` is fine (write to buffer, verify via checksum/markers in uart.log)
2. **Kernel: tiny DT parse helper**:
   - in `neuron/src/arch/riscv/`, extend DT reader to parse `simple-framebuffer` node and publish read-only record (phys addr, size, w, h, stride, format) via bootinfo page or environment handed to userspace
   - keep kernel printk only (no drawing)
   - marker: `neuron: simplefb dt w=1280 h=800 fmt=a8r8g8b8 paddr=0x40000000`
3. **fbdevd service** (`source/services/fbdevd/`):
   - map phys addr from DT via devmem/HAL into shared VMO; expose coherent CPU buffer
   - implement software vsync timer at ~60 Hz (configurable; deterministic tick)
   - coalesce flush requests within tick
   - API (`fb.capnp`): `info()` → `FbInfo`, `map()` → `shm` (VMO id), `flush(rect)`, `vsync()` → `seq`, `fill(rect, argb8888)` (test helper)
   - markers: `fbdevd: ready`, `fbdevd: map ok addr=0x... size=...`, `fbdevd: flush rect x=.. y=.. w=.. h=..`, `fbdevd: vsync seq=..`
4. **windowd integration**:
   - add compositor backend `renderer/backend_fb.rs` that writes premultiplied-alpha ARGB8888 into mapped buffer
   - dirty-rect accumulation per frame; only call `Fb.flush()` for union
   - cursor: small RGBA sprite (e.g., 32×32) blended last; software updates on pointer move
   - VSync pacing: drive render loop from `fbdevd.vsync()`; render only when there are invalidations
   - ensure premultiplied alpha pipeline and sRGB assumption
   - markers: `windowd: backend=fb simplefb 1280x800`, `windowd: frame n=… dirty=(x,y,w,h)`, `windowd: cursor move x=.. y=..`
5. **SystemUI smoke & logo**:
   - add `pkg://assets/branding/nexus_logo_rgba.png` (small)
   - on session start, draw chequered background and centered logo via windowd to validate alpha & scaling
   - marker: `systemui: splash drawn`
6. **CLI diagnostics** (`nx display ...` as a subcommand of the canonical `nx` tool; see `tasks/TRACK-AUTHORITY-NAMING.md`):
   - `nx display info`, `nx display test gradient`, `nx display test rect 10 10 200 120`, `nx display cursor 640 400`, `nx display vsync --count 3`
   - markers: `nx: display info 1280x800`, `nx: display gradient ok`, `nx: display vsync seq=…`
7. **Settings/provider**:
   - seed `settingsd` keys: `display.scale` (already exists), `display.vsync.hz` (int; default 60); provider updates fbdevd's vsync timer
   - marker: `settingsd→fbdevd: vsync=60`
8. **OS selftests + postflight**.

## Non-Goals

- Kernel DRM or kernel display drivers (userspace only).
- Real hardware (QEMU simplefb only).
- HDR support (sRGB only).

## Constraints / invariants (hard requirements)

- **No duplicate framebuffer authority**: `fbdevd` is the single authority for framebuffer access. Do not create parallel framebuffer drivers.
- **No duplicate compositor backend**: windowd simplefb backend extends the renderer abstraction from `TASK-0169`. Do not create a parallel rendering system.
- **Determinism**: framebuffer mapping, vsync timing, and compositing must be stable given the same inputs.
- **Bounded resources**: dirty rect accumulation is bounded; vsync timer is configurable.
- **Device MMIO gating**: userspace framebuffer mapping requires `TASK-0010` (device MMIO access model) or equivalent.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (framebuffer authority drift)**:
  - Do not create a parallel framebuffer service that conflicts with `fbdevd`. `fbdevd` is the single authority for framebuffer access.
- **RED (compositor backend drift)**:
  - Do not create a parallel compositor backend. Extend the renderer abstraction from `TASK-0169` with a simplefb backend.
- **YELLOW (headless vs simplefb)**:
  - `TASK-0055`/`TASK-0170` plan headless present (VMO buffers). This task adds simplefb output. Both can coexist if windowd supports multiple backends, or this task must explicitly replace headless as the canonical path. Document the relationship explicitly.

## Contract sources (single source of truth)

- QEMU marker contract: `scripts/qemu-test.sh`
- Display core: `TASK-0250`
- Renderer abstraction: `TASK-0169` (Scene-IR + Backend trait)
- Windowd compositor: `TASK-0055` (surfaces/layers IPC + vsync)
- Device MMIO access: `TASK-0010` (prerequisite)

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — gated

UART markers:

- `neuron: simplefb dt w=1280 h=800 fmt=a8r8g8b8 paddr=0x40000000`
- `fbdevd: ready`
- `fbdevd: map ok addr=0x... size=...`
- `fbdevd: flush rect x=.. y=.. w=.. h=..`
- `fbdevd: vsync seq=..`
- `windowd: backend=fb simplefb 1280x800`
- `windowd: frame n=… dirty=(x,y,w,h)`
- `windowd: cursor move x=.. y=..`
- `systemui: splash drawn`
- `SELFTEST: fb info 1280x800 ok`
- `SELFTEST: fb gradient ok`
- `SELFTEST: fb cursor ok`
- `SELFTEST: fb splash ok`

## Touched paths (allowlist)

- `pkg://dts/virt-nexus.dts` (extend: simple-framebuffer node)
- `source/kernel/neuron/src/arch/riscv/` (extend: DT parse for simplefb)
- `source/services/fbdevd/` (new)
- `source/services/windowd/` (extend: simplefb backend integration)
- `userspace/libs/renderer/backend_fb.rs` (new; or extend existing)
- SystemUI (splash drawing)
- `source/services/settingsd/` (extend: `display.vsync.hz` provider key)
- `tools/nx/` (extend: `nx display ...` subcommands; no separate `nx-display` binary)
- `source/apps/selftest-client/` (markers)
- `pkg://assets/branding/nexus_logo_rgba.png` (new)
- `docs/display/simplefb_v1_0.md` (new)
- `docs/display/troubleshoot.md` (new)
- `tools/postflight-display-v1_0.sh` (new)

## Plan (small PRs)

1. **DTB & kernel DT parse**
   - DTB: simple-framebuffer node
   - kernel: DT parse for simplefb
   - markers

2. **fbdevd service**
   - framebuffer mapping
   - vsync timer
   - flush coalescing
   - markers

3. **windowd simplefb backend integration**
   - backend_fb.rs integration
   - dirty rect accumulation
   - cursor support
   - vsync pacing
   - markers

4. **SystemUI splash + CLI + selftests**
   - splash drawing
   - `nx display` CLI
   - settings provider
   - OS selftests + postflight

## Acceptance criteria (behavioral)

- `fbdevd` maps and flushes simplefb; vsync timer ticks deterministically.
- `windowd` renders via CPU path with premultiplied alpha and cursor, using dirty-rects.
- SystemUI splash is drawn correctly.
- All four OS selftest markers are emitted.
