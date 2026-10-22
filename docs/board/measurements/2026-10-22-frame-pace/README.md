<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# 2026-10-22 — frame pacing and a scroll frame without corners (TASK-0343, TASK-0335 P4)

## Question

Cycle 61 (`../2026-10-22-frame-chain/README.md`) left scrolling fluid but not followable: a
frame cost the GPU 25 ms on the median against a 16.7 ms refresh, frames landed one and two
periods apart in turn, and each showed the scroll offset of the moment the app ran. The stock
system on the same board scrolls cleanly, in its browser too — so the hardware can. Two questions:
does a scroll frame fit a refresh once it draws only what moved (no corner, no shadow inside the
window, no wallpaper under the glass, no atlas rows cleaned again), and does the content move
evenly once every frame is computed for the time it is shown and the frames come on a steady
cadence?

### What cycle 61's numbers said about a scroll frame

The wheel-scroll windows (`windowd: wheel fwd` around them): one pass, no glass written, 0.92–0.98
Mpx of damage, 1.83–1.95 Mpx of textured pixels (two layers deep), the GPU's wait 16.4–17.1 ms, the
CPU's clean 2.3–2.8 ms a frame. The boot gate prices a program per pixel: opaque 1.34 ns, straight
4.89, rounded 8.69, shadow 10.55; a frame's base ~3.9 ms (the one-draw frame of 0.2 Mpx waited
4.2 ms). Base plus the rounded glass back and one straight layer over the viewport:
3.9 + 0.95 × (8.69 + 4.89) ≈ 16.8 ms — the measured wait. The planner drew a glass's cached
backdrop with the rounded program over the whole glass (never cut into its corners), a layer's
shadow over its whole quad (inside the window the knockout makes it zero), the wallpaper under a
glass (only an opaque layer occluded), and the executor cleaned a scrolled layer's whole source
again every step (a moved source rectangle).

## Instrument

The board's own lines, the operator's series (scroll chat and settings — slow and fast, type a
sentence, drag a window, open and close an app):

- `gpud: gpu work (slot_px= scene_px= blur_px= ns_per_px= opaque_px= straight_px= rounded_px=
  shadow_px=)` — the textured pixels by program (new), beside `gpud: gpu frame us (… clean=
  … wait …)`.
- `gpud: dc flips (… cadence= paced= late=)` — the cadence gpud holds, frames held for their
  slot, landings past it (RFC-0110).
- `windowd: frame pace (on_time= late= early= cadence= period_us=)` — the presents shown when
  windowd predicted (the time the frame pulses named), each second with traffic.
- `windowd: loop … frames(blit=)` while typing; `gpud: dc underflow (n= at_ms= line= taken=
  flips=)` — the first 16 underflows of the boot.
- The judged ends: `gpud: ux frame`, `windowd: ux input`.

| # | Hypothesis | Decision line |
|---|---|---|
| H1 | A scroll frame computes no corner and no shadow | while wheel-scrolling inside a window, `rounded_px` and `shadow_px` ≈ 0 (corners only where the damage touches them) |
| H2 | …and fits a refresh | the scroll windows' GPU `wait` ≤ 10 ms a frame (cycle 61: 16.4–17.1), `gpud: ux frame` ok or warn there |
| H3 | A scroll step cleans only the rows it shows new | `gpu frame us (clean=)` in the scroll windows ≤ 300 µs (cycle 61: 2 264–2 764) |
| H4 | The glass hides what lies under it | in the scroll windows `opaque_px` ≈ the damage (the glass back alone; cycle 61 drew the wallpaper there too) |
| H5 | Frames come on the cadence and land when predicted | `windowd: frame pace` on_time ≥ 90 % of the judged while scrolling; `gpud: dc flips` `cadence=1` in scroll windows, `late=` a small share |
| H6 | The operator follows the content with the eye | the operator's words after scrolling chat and settings |
| H7 | Typing's frames carry pixels | while typing, `frames(blit=)` ≈ 0 a second (cycle 61: 10–21) |
| H8 | When the underflows come | `gpud: dc underflow` lines against flips (`taken=1`), the session's moments (`at_ms`), heavy frames |
| H9 | Nothing breaks | `front=0` in every `dc present us` window, no `FAIL`, the operator sees no seam at a glass's corners and no tear |

## Results

Board cycle 62 (`build/logs/board--2026-10-22T17-29-05`, build `dev-824f`). The operator, after
the series: "everything great except the keyboard and hangs".

| # | Measured | Verdict |
|---|---|---|
| H1 | the nine scroll windows (one pass, no glass written, 0.87–0.95 Mpx of damage): `rounded_px=0 shadow_px=0` in every one | confirmed |
| H2 | the GPU's wait 6.5–8.1 ms a frame (cycle 61: 16.4–17.1), a frame 7.1–11.3 ms with the CPU's share; `gpud: ux frame ok (dropped=0 of=32)` in all nine | confirmed |
| H3 | `clean=` 2 302–2 607 µs in six of the nine, 41 / 971 / 1 118 in the other three | not confirmed — the chat's band takes new strips while it scrolls (windowd copies ~800 rows a second then), and a frame with an upload forgets every cleaned row and cleans the layer's whole source again; next: forget only the rows the upload wrote |
| H4 | `opaque_px` 0.88–0.96 Mpx against 0.87–0.95 Mpx of damage (the glass's back alone), `straight_px` 0.86–0.92 Mpx (the content); nothing under the glass | confirmed |
| H5 | while scrolling `cadence=1` held, `windowd: frame pace` 31–39 on time of 46–56 judged (62–72 %), late 5–7, early 10–16; the session 658 on time of 1 195 (55 %), gpud `late=356` of 1 382 landings | not confirmed (90 %) — the misses while scrolling are mostly early: a frame shown a period before windowd's prediction; to be measured: whether windowd counts a present in flight whose landing gpud already reported |
| H6 | the operator: scrolling chat and settings "great" | confirmed |
| H7 | the 14 windows with keyboard batches: `frames(blit=0)` in all 13 windowd reported (the 14th had no traffic); the session's largest `blit=` is 2 (cycle 61: 10–21 while typing) | confirmed |
| H8 | 2 underflows (cycle 61: 8): `n=1 at_ms=3494 line=26 flips=0` (the scan-out's start), `n=2 at_ms=24352 line=27 taken=1 flips=341` — both in the blanking lines right after a frame boundary, the second with a flip taken there; the operator saw no flicker | when: at a frame's start, with a freshly latched plane; why: open, followed only if the flicker returns |
| H9 | `front=0` in all 37 `dc present us` windows; no FAIL line cycle 61 did not have; no seam, no tear | confirmed |

### The keyboard and the hangs (the operator's two findings)

- **The chat died while typing** — `[USER-PF] LOAD … stval=0xffffffdedededede` (the frame arena's
  poison), in `nexus_dsl_runtime::focus::subtree_is_secure::walk` (resolved through the linker map
  of a byte-identical rebuild of the shipped app-host). Cycle 61 has the same crash. Before it
  `apphost: text commit ERR Malformed` twice; in settings four taps answered
  `apphost: tap dispatch ERR Malformed` — a tap that does nothing, which reads as a hang.
- **Cause, measured on the host:** an app reads its program through ONE capnp reader for its whole
  life, and capnp's traversal budget (`TRAVERSAL_LIMIT_WORDS`, 4 MiB) belongs to the reader — every
  emit and every dispatch spends from it, nothing refills it. Once spent, every read is
  `Malformed`. Settings toggling its window menu answers `Malformed` after 28 toggles, the greeter's
  field after 482 keystrokes; with eight times the budget after 260 and 3 886 — linear in the budget.
- **Why it then crashes:** a failed emit has already opened its arena generation — which resets the
  one two back — but keeps the scene it failed to replace; the next generation resets the scene
  still shown, and the next tap's focus walk reads it.
- **A second hang:** right after `uitheme: switched` the app's input hop reads up to 0.78 s
  (`apphost: hops input (rx>sent=…/776823/16)`) — every window re-rasterised in full on the CPU.
- Next package: a program read bounded per operation, not per session; a frame that fails never
  resets the frame on screen (the arena's generation rolled back).
