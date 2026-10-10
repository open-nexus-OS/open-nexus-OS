<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# 2026-10-10 — the GPU on the stock system: its state while running, and what a frame costs there

## Question

Two things before the GPU lane writes code (G0 of `tasks/TASK-0329`, and the comparison the
frame-time gate is set against):

1. What does the GPU look like while the stock system runs it — the node, the clock, the
   power domain, the firmware, the registers, the scheduling configuration?
2. What does a frame of OUR scene cost on this GPU through the stock system's own driver —
   the number our driver is measured against?

## Instrument

The stock system (vendor kernel 6.6.63) over adb as root. Read-only: `/proc/device-tree`,
`/proc/interrupts`, the clock and power-domain summaries and the GPU driver's own status,
version, power-timing and debug-dump files under debugfs. One program, `gate_scene.py`, run
from the volatile `/tmp` and removed afterwards (operator decision 2026-10-10: a test program
on the running system is allowed; nothing on the device's storage is changed). Raw dumps are
not archived (they carry vendor text); the register values are, as data, in
`gpu-registers-running.txt`.

`gate_scene.py` draws our scene through the system's GLES stack into an off-screen 1920x1080
target, every frame from scratch, and waits for the GPU after each frame:

| Scene | What it draws |
|---|---|
| `wall` | one full-screen textured quad |
| `glass` | `wall`, then three glass layers (a bar 1920x40, a dock 800x90, a window 960x1044): per layer a separable gaussian backdrop blur of its rectangle at full resolution (two passes, 21 taps), a soft rounded shadow, a translucent rounded-rect content quad |
| `glass-small` | `glass` with the bar and the dock only |
| `glass-quarter` | `glass`, but the backdrop is one quarter-size copy of the frame, blurred once; each layer samples it |

## Results

**The device as the stock system runs it**

- Node `compatible = "img,rgx"`, window `0xcac0_0000`/`0x80000`, interrupt 75, one clock, one
  reset, power domain 2, and **`dma-noncoherent`** on the node.
- Core id register `CORE_ID__PBVNC = 0x0024001D003400B6` — 36.29.52.182, one core.
- Clock 614.4 MHz, enabled; no frequency change since boot. Power domain on, device active.
- Firmware: two files for this core (139 264 and 274 840 bytes; sha256 `860000d5…` and
  `43b9110b…`), firmware and driver both report release 24.2, build 6603887. This is NOT
  the build of the published, redistributable firmware for the same core (1.1, build
  6976702, 126 976 bytes) whose host interface the open kernel driver documents.
- Power-on, as the driver timed itself: 155 µs for the system step, 226 µs for the device
  step; firmware boot 354 of its timer ticks.
- Scheduling as configured: a medium context-switch profile; context switching enabled for
  the transfer, geometry, fragment and compute data masters; one OS slot, priority 0, no time
  slice. The system's EGL offers a context-priority extension.
- Watchdog 3 s, hardware-recovery timeout 1 s; zero recovery events.
- MMU: catalogue base slot 1 in use (`BIF_CAT_BASE1 = 0x8BE38000`, index register `0x101`),
  the others zero.
- All GPU interrupts on hart 0.
- Memory at the login screen: 186 MB of GPU memory allocated, 94 MB mapped, 0.7 MB of page
  tables.

**The frame** (hardware renderer, 200–300 frames a scene after 30 warm-up frames; two runs)

| Scene | median | p95 | p99 | GPU interrupts per frame |
|---|---|---|---|---|
| `wall` | 5.0–5.3 ms | 5.8–12.4 ms | 7.7–46 ms | 2.1–2.7 |
| `glass` | 59.5–60.1 ms | 70 ms | 77–82 ms | 17–22 |
| `glass-small` | 12.9 ms | 13.5 ms | 13.8 ms | 10 |
| `glass-quarter` | 23.3 ms | 23.4 ms | 23.6 ms | 10 |

For comparison, our CPU path on the same board and day: 150 ms median a present
(`../2026-10-10-mgs-baseline/`).

**What is the driver's share and what is the hardware's** (`sweep.py`, one draw per frame at
growing sizes; 200 frames each, two GPU interrupts a frame throughout)

| Frame | median |
|---|---|
| textured quad 16x16 | 0.64 ms |
| textured quad 240x135 | 0.71 ms |
| textured quad 480x270 | 0.87 ms |
| textured quad 960x540 | 1.58 ms |
| textured quad 1920x1080 | 4.43 ms |
| the same, blended | 7.17 ms |
| three blended full-screen quads | 10.75 ms |
| three OPAQUE full-screen quads | 4.62 ms |
| one 21-tap blur pass 480x270 | 3.26 ms |
| one 21-tap blur pass 960x540 | 11.21 ms |
| one 21-tap blur pass 1920x1080 | 42.77 ms |

Read as intercept and slope:

- **Fixed cost of a frame through the stock stack: 0.64 ms** — submit, two interrupts, wait.
  This is the part a driver and a system own.
- **Opaque textured fill: 1.8 ns a pixel** (about 550 million pixels a second) — the
  hardware's.
- **Opaque overdraw is free** (three opaque screens cost what one costs): hidden pixels are
  removed before shading.
- **A blended full-screen layer: 2.7 ms for the first, 1.8 ms for each further one.**
- **A texture fetch: about 1 ns** (a 21-tap pass costs 20.6 ns a pixel).

## Verdict

1. **This GPU does not redraw a glass desktop at 1920x1080 in a 60 Hz frame — not through
   the stock driver either.** One full-screen textured quad costs 5 ms: about 400 million
   simple pixels a second, so a 16.7 ms frame holds about three full screens of simple fill.
   The straightforward glass scene costs 60 ms; with the blur done once at quarter size it
   costs 23 ms. A driver of ours cannot be faster than the hardware.
2. **The compositor's end form on this board is therefore decided by the hardware, not by
   taste:** it redraws what changed, never the frame.
   - Partial update of the scanned ring (buffer age): a frame costs its damage.
   - The blur of a glass layer is a cached, reduced-size surface that is redrawn only when
     what lies under it changed (the cache the virgl path already keeps).
   - What moves often and alone goes to a plane of the display controller (the pointer is
     there already; a dragged window, a video, the scrolled surface are the candidates).
   - Opaque content is drawn opaque and front to back; a tile-based renderer removes hidden
     opaque pixels for free and pays for every blended one.
   - A render target is not read back in the frame that draws it: the 17–22 interrupts a
     frame of `glass` are the price of switching targets mid-frame.
3. **Where our driver is measured against the stock one, and where it cannot differ.** The
   per-pixel and per-fetch costs are the silicon's; a driver that reaches them is at the
   limit. What a driver and a system own is the fixed cost of a frame and what the frame asks
   of the hardware. The gates, each with the stock number as the bar:
   - fixed cost of a frame (the 16x16 frame): **below 0.64 ms**;
   - slope: opaque fill at or below 1.8 ns a pixel, a blended layer at or below 1.8 ms a
     full screen — equal is the pass, above it is a driver or shader-code fault;
   - the scene: `wall` at or below 4.4 ms, and our compositor's own frame of the same
     picture **below** the 23.3 ms of `glass-quarter` — by asking less of the hardware
     (damage, cached blur, planes), which is where the system can be better.
4. **Coherence:** the tree says non-coherent. Every buffer shared with the GPU needs its
   declared cache mode (requirement 4 of `tasks/TASK-0329`).
5. **Firmware decision:** two candidates exist. The published firmware has an open host
   interface and a licence that allows unmodified redistribution; the stock firmware is the
   one proven on this board and has neither. G1 starts with the published one; the stock
   one's register values in `gpu-registers-running.txt` are the expected values our bring-up
   compares against, where the two agree.

## Not measured here

- The register window with the domain off and during power-up (the sequence itself): G1's
  first cycle, with socd's bring-up line.
- Submit-to-completion inside the driver (firmware trace): not enabled on the stock system;
  the frame times above include it.
- Preemption between contexts under load: the configuration is read, the behaviour is not.
