<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# 2026-10-22 — program reads per operation, a failed frame abandoned (TASK-0344)

## Question

Cycle 62 (`../2026-10-22-frame-pace/README.md`, § the keyboard and the hangs): the operator
"everything great except the keyboard and hangs". The chat died while typing (a user page fault
on the frame arena's poison in the focus walk, cycles 61 and 62), settings answered taps
`tap dispatch ERR Malformed`. Measured on the host: an app read its program through one capnp
reader whose traversal budget was spent over the app's life — `Malformed` after 28 menu toggles in
settings, 482 keystrokes in the greeter, linear in the budget — and a failed emit kept its
generation, so the next one reset the scene still on screen. A second session on the cycle 62
image added the third cause: the chat died of `alloc-fail` with its 4 MiB heap full — every
keystroke re-lays out the chat page, its boxes spill from the frame arena to the base heap as
blocks above the largest size class, and those were never reused (~155 KB a keystroke, dead after
~20). With a reader per operation, a failed frame abandoned and large blocks reused: does every tap
and keystroke answer however long the session runs, does nothing read the poison, and does the
heap stay flat while typing?

## Instrument

The board's own lines, the operator's series — a long one this time: type a few sentences in the
chat (more than a hundred keystrokes), open and close settings' menus more than thirty times, tap
around, switch the theme once:

- `apphost: text commit ERR`, `apphost: text action ERR`, `apphost: tap dispatch ERR` — any `ERR`
  of the runtime;
- `[USER-PF]` in an `app.probe`, `execd: crash report … name=app.probe`;
- `apphost: frame arena (… large=blocks/bytes lost= carve=class/large abandoned=)` — the large
  list, frames that failed and were discarded (new), the base heap's growth per sample;
  `apphost: heap steady`; `alloc-fail`;
- `apphost: hops input (rx>sent=)` — the app's answer, and after `uitheme: switched` the theme
  switch's full raster (not this package's; measured again as the baseline for its follow-up).

| # | Hypothesis | Decision line |
|---|---|---|
| H1 | No app runs its program reads dry | no `ERR Malformed` in the session (cycle 62: 2 text commits, 4 taps) |
| H2 | Nothing reads a frame that was reset | no `[USER-PF]` in an `app.probe` (cycles 61, 62: one each, while typing) |
| H3 | Frames do not fail in a normal session | `abandoned=0` in every `frame arena` line — or each failed frame named by its `ERR` line |
| H4 | Typing in the chat keeps the heap flat | after the chat's first layouts `carve=…/0` while typing, `apphost: heap steady` printed in the chat, no `alloc-fail`, `lost=0` (cycle 62's second session: `carve=0/636928` every four layouts, `alloc-fail` after ~20) |
| H5 | The keyboard and the taps answer | the operator's words after the series; `apphost: hops input` worst under 200 ms outside the theme switch |
| H6 | The theme switch is a separate cost | the input hop after `uitheme: switched` (cycle 62: up to 0.78 s) — recorded for the follow-up |

## Results

Board cycle 63 (`build/logs/board--2026-10-22T18-51-01`, build `dev-8693`). The session: greeter,
calculator, the chat twice (typing in the second: 53 keyboard batches, the chat's layout counter
from 1 to 28 — the old image died at ~20), settings and stash briefly, the control center, the
theme switched to light.

| # | Measured | Verdict |
|---|---|---|
| H1 | no `ERR` of the runtime in the session — no `text commit`, `text action` or `tap dispatch` error (cycle 62: 6) | confirmed |
| H2 | no `[USER-PF]` in an app, no app crash report (the two are the self-test's `demo.fault` and `demo.minidump`); every app stopped with `code=0` | confirmed |
| H3 | `abandoned=0` in every `frame arena` line | confirmed |
| H4 | the chat (`spill=1`): `base=674096` from layout 4 to 24 with `carve=0/0` while typing, `large=6–7` blocks parked, `lost=0`; at layout 28 `carve=23168/0` (small classes, once); `apphost: heap steady` 31 times; no `alloc-fail` (cycle 62's second session: `carve=0/636928` every four layouts, dead after ~20) | confirmed |
| H5 | typing in the chat: the app's input hop 17–21 ms on average, worst 123–152 ms; a window with taps into the text field 464 ms; the desktop shell opening its control center 654 ms — a full CPU raster of a large panel; `windowd: ux input` ok=48 warn=14 FAIL=4 | the answers come (no dead tap, no dead key); their latency where a large area is re-rastered is the follow-up's — the operator's words pending |
| H6 | the shell's hop around `uitheme: switched` 742 ms (cycle 62: 776 ms) | unchanged, recorded for the follow-up |

Settings' long session (H1's 30 menu toggles) was not run on the board this cycle; the host's
`a_long_session_never_runs_the_program_reads_dry` (1 000 toggles) carries it.
