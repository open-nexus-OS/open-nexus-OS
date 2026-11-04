<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# 2026-11-03 — what a motion costs the app, where a tap's time goes, what a frame holds (TASK-0351 P0, TASK-0354 P1, TASK-0355 P0)

## Question

The operator reports the top bar and the launcher as slow and every in-surface transition as not
smooth. Nothing had priced them: the frame's verdict existed (`gpud: ux frame`), the input's end
existed (`windowd: ux input`), but no line said how often an app wakes for a transition, how long
its frame takes from the pulse to its send, how long an input waits in windowd's staging stage,
how many layers a frame composes or how many of a board GPU context's bindings are in use. One
board session answers the three measurement phases at once, before any cut.

## Instrument

New lines of this package (RFC-0108 §hops, numbers only), read from `uart.log` after the session
(`just board-log`), with the lines that existed:

- `apphost: hops motion (pulse>sent=<avg_us>/<max_us>/<count> ticks=<wake-ups>)` — per motion
  episode (motion live from a frame pulse until a pulse finds none); the hop is a pulse's arrival
  to the frame that answers it, `ticks` the pulses that found motion live.
- `windowd: hops input (staged>applied=…)` — an input's wait in the staging stage until the frame
  applies it.
- `gpud: gpu work (… layers=)` and `gpud: bindings (in_use= of= contexts=)` — layers per frame,
  bindings per board GPU context.
- The ends: `gpud: ux frame`, `windowd: ux input`; `windowd: hops frame (owed>sent= sent>acked=)`.

The operator opened four apps (calculator, chat, settings, stash twice), the six top-bar panels,
the launcher small and big, paged it, and opened and closed windows. The 12- and 30-window series
of TASK-0351 P0 was not run in this session (at most 4 windows at once).

| # | Hypothesis | Decision line |
|---|---|---|
| H1 | A Tier-2 transition wakes the app once per shown frame and the frame is cheap | `hops motion`: `ticks` ≈ `count`, `pulse>sent` avg ≤ 16 667 |
| H2 | The input's time is in the staging stage (the frame applies inputs late) | `hops input` avg a sizable share of `ux input` `to_submit_us` |
| H3 | The input's time is in the display's frame (gpud) | `hops frame` `sent>acked` ≫ `owed>sent` and ≫ `hops input` |
| H4 | The compose is far from its budget already at a handful of windows | `ux frame` dropped per 32 at 4 windows; `layers=` against the 64-layer budget; `bindings` against 128 |

## Results (`board-2026-11-03T19-13-30-lines.txt`, image `dev-f9520ca9`)

| # | Verdict | The lines |
|---|---|---|
| H1 | **fails on both counts** — the app wakes ~10 times per frame it sends, and a transition frame costs 77 ms | 52 episodes, 4 776 wake-ups, 462 answered hops: 10.3 wake-ups per sent frame; `pulse>sent` median of the episode means 77 ms, weighted 114 ms, worst 6.4 s; 42 of 52 episodes woke more often than they presented |
| H2 | **no** — staging is free | `staged>applied` 7 688 inputs, weighted mean 55 µs, worst 8.8 ms — against `to_submit_us` median 77 ms |
| H3 | **holds** — the display's frame holds the tap's time | `hops frame` `sent>acked` mean 90 ms (worst 396 ms), `owed>sent` mean 16 ms (worst 271 ms); `ux input` 15 ok / 20 warn / 20 FAIL of 55 windows, median 77 ms |
| H4 | **holds** — the frame is red at 4 windows | `ux frame` 26 FAIL / 1 warn of 27 windows, dropped median 24 of 32, worst frame median 102 ms; `ns_per_px` median 6 (cycle 61: 13), `layers` median 16, max 19; `bindings` in_use max 49 of 128, 1 context; `windowd: loop hz` median 187 |

**What this says for the three tasks.**

- TASK-0354 P1: a Tier-2 transition is a 10-fps thing on the app side — 77 ms from the pulse to
  the sent frame, ten pulses queued and handled as wake-ups in between. The motion belongs on the
  compositor (P2) and the app out of the frame loop; the per-episode line is the before-and-after
  instrument. The lines do not name which transition (panel, launcher, window) an episode was: that
  attribution comes with windowd's motion record (P2), which knows the layer.
- TASK-0355 P0: the dominant hop of a tap is gpud's frame, `sent>acked` 90 ms mean — not the
  staging stage (55 µs) and not windowd's own work (`owed>sent` 16 ms). Its owner is TASK-0351 (the
  frame's passes and per-pixel cost); P1 of this task waits for that cut.
- TASK-0351 P0: at 4 windows the frame already drops 24 of 32 at 6 ns/px over ~2.8 Mpx of slot
  area and 16 layers; bindings are at 49 of 128 in one context. The 12- and 30-window series is
  still owed and opens package 3.
