---
title: TASK-0340 windowd rewrites no retained plane where the GPU composes — the 120 µs-a-row CPU work cycle 55 measured, gone from every frame
status: In Progress (built 2026-10-21; board cycle 56: no retained row, windowd's flush 30 → 2.2 ms median — the commit next)
owner: @ui
created: 2026-10-21
depends-on: [TASK-0339 (the work split that measured it)]
follow-up-tasks: [zero-copy client surfaces — the GPU samples the app's buffer, windowd copies nothing (RFC-0101 §7b extension; the 37 µs-a-row atlas copy goes with it), chat's surface VMO at launch (`vmo_ms` 0.7–0.9 s, layer 6b commit-on-touch)]
links:
  - Measurement: docs/board/measurements/2026-10-21-gpu-vendor-per-pixel/README.md ("Cycle 55": `work(…)`)
  - ADR: docs/adr/0071-screen-capture-readback-in-gpud-freeze-in-windowd-ui-in-the-shell.md (the freeze copies the frame into the retained plane — gpud's)
  - Code: source/services/windowd/src/compositor/{gpu_context.rs,mod.rs}, source/services/windowd/src/compositor/runtime/{present.rs,desktop_surface.rs}
---

## Context (cycle 55)

windowd's new `work(…)` field split its own CPU time per frame: for every content rectangle it
rewrote the wallpaper into the retained plane — a scaled copy and one `vmo_write` per row, 120 µs
a row on the board, 95–220 ms for a window-sized frame, 10.2 s of its CPU in one session — while
the GPU executors never read that plane outside a capture: the board's executor composes from the
wallpaper plane and reads the retained plane only as a frozen capture's frame, which gpud copies
there itself (`READBACK_FREEZE`); QEMU's virgl samples its wallpaper texture (the freeze replaces
it, the thaw's `OP_WALLPAPER_DIRTY` re-uploads it). Only the CPU 2D executor blits the plane.

## Goal

No retained-plane rewrite where the GPU composes; the CPU 2D executor keeps its plane.

## Non-Goals

- The atlas copy (37 µs a row): its end is zero-copy client surfaces, not a cheaper copy.
- The capture freeze (gpud's, unchanged).

## Plan / what was built (2026-10-21)

- `gpu_context::open()` answers whether windowd's GPU context opened (the GPU composes);
  windowd keeps it (`set_gpu_composes`).
- `write_rows` / `write_damage_rect` skip the rewrite where the GPU composes and keep their
  bookkeeping (`retained_plane_unread`) — the first frame, every flush and the thaw follow one
  rule, the path the frozen capture already took for every flush.
- The desktop surface's copy is counted in `work(copy_rows=` with the windows'.

## Proof

- QEMU `headless` (the 2D executor, `gpu context none`): the plane is still written
  (`base_rows=800`); `visible` (virgl, `gpu context ok`): `base_rows=0`, the slowest flush 23–66 ms
  instead of 0.37–0.65 s (TCG).
- Board cycle 56: `work(base_rows=0` and `flush_us_max` against cycle 55's 30 ms median / 254 ms
  max; `windowd: ux input` against cycle 55; a screenshot still freezes and thaws correctly.

## Results

- 2026-10-21: built; host tests, `just check`, `just diag`, `build-os-workspace` (0 warnings),
  QEMU headless + visible green as above.
- **Cycle 56 (board, 2026-10-21):** `work(base_rows=0` over the whole session (cycle 55: 84 561
  rows, 10.2 s); windowd's slowest flush per second 2.2 ms median (30 ms), 198 ms max (the atlas
  copy: 102 234 rows, 4.5 s); `windowd: ux input` median of the window maxima 842 ms (1 147), max
  2.8 s (9.2 s); a screenshot froze and thawed. Frames still 75 % over 16.7 ms — the GPU's frame:
  the scroll body is composed as glass (3.93 Mpx a scroll step on the host model, 1.58 Mpx opaque)
  and a hover is an app re-render — the next package. The board's `SELFTEST: rss loop FAIL` was a
  launch during the loop's window; the rung now judges the smallest step of its ten cycles.
