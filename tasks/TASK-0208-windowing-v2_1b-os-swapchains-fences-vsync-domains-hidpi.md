---
title: TASK-0208 Windowing/Compositor v2.1b (OS/QEMU): swapchain surfaces (VMO) + acquire/release timeline fences + vsync domains + HiDPI v1 + timings overlay + nx-win + selftests/docs
status: Draft
owner: @ui
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Windowing v2 host substrate: tasks/TASK-0207-windowing-v2_1a-host-surfacecore-swapchain-fences-hidpi.md
  - Windowing/Compositor v2 OS integration: tasks/TASK-0200-windowing-compositor-v2b-os-wm-lite-alt-tab-screencapd.md
  - Present scheduler + input routing baseline: tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md
  - windowd compositor surfaces baseline (VMO): tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md
  - Renderer/windowd wiring baseline: tasks/TASK-0170-renderer-abstraction-v1b-os-windowd-wiring-textshape-perf-markers.md
  - Driver contracts (future GPU): tasks/TRACK-DRIVERS-ACCELERATORS.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

With the host-first v2.1 surfacecore contract defined, we wire a GPU-ready surface model into OS `windowd`:

- swapchains (VMO-backed image sets),
- acquire/release **timeline** fences (simulated; no GPU),
- vsync domains (still timer-driven in QEMU),
- HiDPI v1 (dp coordinates for clients; px compositor),
- timings overlay + CLI tools for observability and deterministic proof.

This must remain QEMU-tolerant and kernel-unchanged.

## Goal

Deliver:

1. OS surface API changes (windowd + IDL as needed):
   - register a surface with a swapchain descriptor and image buffers (VMOs)
   - `acquire` returns an image index + acquire fence id
   - client submits an image index + damage + epoch
   - compositor latches only when acquire fence is signaled
   - compositor returns per-surface release fence after present latch
   - markers:
     - `surface: create swapchain images=<n> w=<w> h=<h>`
     - `windowd: latch n=<k>`
     - `windowd: release fence id=<id>`
2. Vsync domains:
   - config defines domain list (id/hz)
   - windowd composes per-domain tick
   - marker:
     - `windowd: vsync dom=<id> seq=<n>`
3. HiDPI v1:
   - per-display scale factor from allowlist
   - input hit-testing and WM computations operate in dp and map deterministically to px
   - marker:
     - `windowd: display scale=<s>`
4. Renderer integration:
   - read from swapchain image buffers (VMO mapped) in premultiplied RGBA
   - damage regions expanded dp→px deterministically
   - fence coupling: present returns release fences
   - marker:
     - `renderer: compose dom=<id> tiles=<n> dmg_regions=<k>`
5. Timings overlay:
   - SystemUI overlay shows per-domain ring buffer (120 frames):
     - acquire→latch, compose, present
   - NOTE: do not gate pass/fail on absolute microseconds; overlay is diagnostic
   - markers:
     - `ui: frame overlay on`
6. `nx-win` extensions (host tool):
   - `displays`, `scale set`, `frame-stats`
   - NOTE: QEMU selftests must not require running host tools inside QEMU
7. OS selftests (bounded):
   - create swapchain and wait for release fence completion:
     - `SELFTEST: surf fences ok`
   - show overlay and ensure pacing skip occurs on idle (via counters/markers, not time):
     - `SELFTEST: frame pacing ok`
   - set scale and verify dp→px mapping effect via geometry markers:
     - `SELFTEST: hidpi ok`
   - thumbnails still work (consume last presented swapchain image):
     - `SELFTEST: thumbnails ok`

## Non-Goals

- Kernel changes.
- Real GPU backend; fences are simulated timeline counters until driver stack exists.
- True multi-display output (domains are logical).

## Constraints / invariants (hard requirements)

- No fake “GPU-ready” claims: swapchains/fences are contract-level, not GPU proof.
- No raw pointers across IPC; OS uses VMO/filebuffer mapping only.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p windowing_v2_1_host -- --nocapture` (from v2.1a)

- **Proof (QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=195s ./scripts/qemu-test.sh`
  - Required markers:
    - `SELFTEST: surf fences ok`
    - `SELFTEST: frame pacing ok`
    - `SELFTEST: hidpi ok`
    - `SELFTEST: thumbnails ok`

## Touched paths (allowlist)

- `source/services/windowd/` + IDL
- `userspace/libs/surfacecore/` (OS adapter)
- `userspace/libs/renderer/` (swapchain sources + dp→px)
- SystemUI overlays (timings)
- `tools/nx-win/`
- `source/apps/selftest-client/`
- `schemas/windowing_v2_1.schema.json`
- `docs/windowing/` + `docs/tools/nx-win.md`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. windowd swapchain registration + acquire/submit/latch/release (simulated timeline fences)
2. vsync domains + pacing rules + markers
3. HiDPI dp↔px mapping + input/WM adjustments
4. renderer swapchain source + fence coupling
5. timings overlay + selftests + docs + postflight wrapper (delegating)

## Acceptance criteria (behavioral)

- In QEMU, swapchain/fence lifecycle, pacing behavior, and HiDPI mapping are proven deterministically by selftest markers.

Follow-up:

- Compositor v2.2 (gpuabst stubs + async present + plane planner + cursor plane + basic color spaces + metrics/CLI) is tracked as `TASK-0215`/`TASK-0216`.
