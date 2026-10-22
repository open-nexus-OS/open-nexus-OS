<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# 2026-10-22 — the frame chain: one display interrupt a frame, and where a chain's time goes (TASK-0342)

## Question

Cycles 55–59 composed ~3 frames a second while anything moved. Cycle 59
(`../2026-10-21-gpu-vendor-per-pixel/README.md`, "Cycle 59") measured a frame's wait from
windowd's send to gpud's credit at 631 ms (median; worst 2.8 s) against 18–60 ms of GPU work, and
an interrupt storm in gpud: `gpud: dc flips (n=384 … wakes=302883 late=257131)`, ~790 blank wakes
per flip, 85 % outside the blank — six display sources left live by the mask, only the vsync
cleared by each wake. Two questions: does one interrupt a frame give the frames back, and once it
does, which hop of the input and the frame chain holds the rest of the time?

## Instrument

The board's own lines, operator session (scroll in chat and settings, type a sentence, drag a
window, open and close an app):

- `gpud: dc flips (n= pending= crossed= line_after_max= wakes= late= vsyncs= unraised= onl2=0x
  out=0x)` every 64 flips — the wakes and what raised them (the vsync, neither word, every source
  seen in the pipeline's and the output control's raw words).
- RFC-0108 §hops, `<from>><to>=<avg_us>/<max_us>/<count>`: `gpud: hops frame (recv>drawn=
  drawn>shown=)` every 32 frames on screen; `windowd: hops frame (owed>sent= sent>acked=)` each
  second with traffic; `apphost: hops input (rx>sent=)` every 16 inputs that asked for work.
- The judged ends: `windowd: ux input`, `gpud: ux frame`; windowd's loop line
  (`damage(flushed_px=)`, `work(copy_us=)`) for typing.

| # | Hypothesis | Decision line |
|---|---|---|
| H1 | The storm was the six live sources: held back, a flip costs one or two wakes | `dc flips`: `wakes` / `n` ≤ 2, `late` a small share |
| H2 | Another group's source shares gpud's line (if H1 fails) | `unraised` ≈ `wakes` |
| H3 | The output control's own sources raise the line (if H1 fails) | `out=` ≠ `0x0` |
| H4 | With one interrupt a frame, a frame's credit comes back within its work plus two frames | `windowd: hops frame` `sent>acked` avg ≤ `gpud` `recv>drawn` avg + 33 000 |
| H5 | Then the GPU's work paces the frames → steps 4–6 first; if the wait for the blank does → step 3 first | `gpud: hops frame`: `recv>drawn` against `drawn>shown` |
| H6 | A typed character re-renders its field, not the page | while typing, `flushed_px` per frame ≪ 2 073 600 and `apphost: hops input` avg ≤ 50 000 |

## Results

### Cycle 60 (2026-10-22, image `dev-e50bf0c8`, `board--2026-10-22T12-25-11`)

Lines: `cycle60-lines.txt` (every gpud line of the chain, the ux verdicts, every sixth windowd hop
line). The operator scrolled chat and settings, typed a sentence, dragged and tiled a window,
opened and closed apps.

| # | Verdict | The lines |
|---|---|---|
| H1 | **fails** — the storm is untouched | `dc flips (n=256 … wakes=609354 … vsyncs=609354 unraised=0 onl2=0x21f out=0x1f)`: ~2 400 wakes a flip, and the raw vsync bit set at **every** wake — nothing the wakes wrote cleared it |
| H2 | no | `unraised=0` |
| H3 | not decisive | `out=0x1f` (both outputs' start-of-frame, eof, cfg-eof, cfg-eol, underflow) — latched since the boot, never cleared |
| H4 | the credit waits for the flip | `windowd: hops frame` `sent>acked` median 777 ms (worst 4.0 s); gpud `drawn>shown` median 723 ms (worst 3.9 s) against `recv>drawn` 37 ms |
| H5 | **the wait for the blank holds the frame** → step 3 first | `drawn>shown` 723 ms ≫ `recv>drawn` 37 ms (the GPU's frame 37 ms, its wait 35 ms) |
| H6 | **holds** | after the first text commit a keystroke's present names one rectangle of 88 320 px, 92 rows copied (`damage(rects=1 queued_px=88320)`, `copy_rows=92`) — not the window; `apphost: hops input` `rx>sent` median 65 ms (the previous present's credit included) |

The judged ends stay red: `gpud: ux frame FAIL` 8 of 8 windows, `windowd: ux input FAIL` 16 of 16;
launches 2 ok, 4 warn, 2 FAIL.

**Why H1 failed — read from the vendor kernel's register header (reference only).** The block at
`0x900` holds three words per pipeline with one bit layout (bit 0 vsync, 1 eof, 2 cfg-eof, 3
cfg-line, 4 cfg-ready-clear, 9 underflow, 12–25 the command list's channels): the ENABLE word
`0x910` (`onl2_nml_*_int_msk` — a set bit lets the source raise the line; the vendor's
`irq_enable` writes 1 to turn a source on), the latched STATUS `0x938` (`*_int_sts`, cleared by
writing its bits) and the RAW word `0x960` (`*_int_raw`, read-only). The vendor's handler reads
the raw word and clears the status. Ours read `0x910` as a mask: "every source but the vsync held
back" (`0xfffe`) turned on every source but the vsync, "disabled" (`0xffff`) turned on all of
them, and every clear went to the read-only raw word — the status never fell, the level line
stayed up. The 2026-10-12 record read the same word backwards (`../2026-10-12-scanout-vblank/`,
correction appended there). The output control's block has the same three words at `+0xb0`
(raw), `+0xb8` (enable; the stock `0x252` turns on both outputs' eof and underflow) and `+0xc0`
(status).

**And why fixing it alone would not help.** One interrupt a frame at the vsync gives gpud the
45-line blank (~0.67 ms) to write the flip — while gpud spends ~35 ms of every frame in its GPU
wait. The storm made flips land at all: a wake every ~230 µs fell into some blank. The vendor's
commit path writes the plane at any time and sets only the configuration latch; the controller
takes it at its next frame boundary (the start word only at the first frame).

### Cycle 61 (built 2026-10-22): the interrupt words as the header names them, flips through the latch

| # | Hypothesis | Decision line |
|---|---|---|
| H7 | With the latch-taken and vsync sources alone enabled and the status cleared, a flip costs one or two wakes | `dc flips`: `wakes` / `n` ≤ 2, `unraised=0`, `held` a small share |
| H8 | A latched flip lands within two frames of being drawn | `gpud: hops frame` `drawn>shown` median ≤ 33 000 |
| H9 | The GPU's frame now paces the frames: ~25 a second while something moves | `windowd: loop … present=` median ≥ 20; `sent>acked` ≈ `recv>drawn` + `drawn>shown` |
| H10 | A flip written mid-scan never tears (the latch) | the operator sees no tear line while dragging and scrolling; no `gpud: FAIL dc present front` |
| H11 | The latch-taken source fires once a flip (the vsync could go) | `taken` ≈ `n`; `at_wake` against `at_pass` |

### Cycle 61 (2026-10-22, `board--2026-10-22T12-58-39`)

Lines: `cycle61-lines.txt` (every gpud line of the chain, the ux verdicts, every eighth windowd
loop and hop line). The operator: "a quantum leap in performance — scrolling is fast now, fluid,
but I cannot follow the messages with my eyes; now and then a flicker, not repeatable; the
keyboard input was slower".

| # | Verdict | The lines |
|---|---|---|
| H7 | **holds** | `dc flips (n=2432 at_wake=2336 at_pass=96 behind=1703 wakes=2508 held=82 taken=2493 vsyncs=2493 unraised=0 underflows=8)`: 1.03 wakes a flip (cycle 60: ~2 400); `dc vblank ok (min_period_us=15785 …)` — never two wakes inside a frame |
| H8 | **holds** | `drawn>shown` median 22 ms (cycle 60: 723 ms) |
| H9 | **holds at the median** | windowd `present=` median 20 a second (cycles 55–60: ~3), max 52; `sent>acked` median 56 ms ≈ `recv>drawn` 25 ms + `drawn>shown` 22 ms + the queue (`behind`: 70 % of the presents waited behind a latched flip) |
| H10 | **no tear seen; a flicker is** | `front=0` in all 76 windows; the operator saw no tear line — and a rare flicker: **8 scan-out underflows** (`underflows=8`, spread over the session: 6 in its first 400 flips, the rest single) — the display's read channel starved for a frame |
| H11 | **holds** | `taken=2493` of 2508 wakes: the latch-taken source fires once a flip, together with the vsync |

The judged ends: `windowd: ux input` ok 103, warn 33, FAIL 17 (cycle 60: FAIL 16 of 16);
`gpud: ux frame` ok 4, warn 5, FAIL 67 — the GPU's frame is now the pace: median 24.7 ms (p10
16.0, p90 40.7), frames of three or more passes 33.6 ms against 18.2 ms at one, per pixel 13 ns
(p50; the vendor's compositor 1.9–3.7).

**Why scrolling is fluid but cannot be followed.** A frame takes 25 ms of GPU on the median, a
refresh is 16.7: frames land one and two refreshes apart in turn, and each shows the scroll offset
of the moment the app computed it, 40–80 ms before it reaches the glass — the content moves in
uneven steps (judder). The reference platforms' answer is twofold: a frame within the refresh
(the GPU's cost — steps 4–7) and frame pacing (a steady cadence, the motion computed for the time
the frame will be shown).

**Why typing lags.** While typing, every frame re-blurs a glass whose backdrop the edited text
changes (`glass writes (band=29–30)` per 32 frames, 3.7–3.8 passes, 33 ms, 19–21 ns a pixel), and
windowd sends 10–21 frames a second that change no pixel (`frames(blit=10–20)`,
`damage(queued_px=0 flushed_px=0)`; queued by `queue_gpu_blit_rect` — its animation ticks or a
window-manager move; which of them is cycle 62's to name), each a full GPU frame the keystroke's
frame may queue behind.

**The flicker.** The read channel's control word is the stock one in effect (`0x202040`:
16 outstanding, burst 16; the stock `0x1e203c`: 15 and 15), the display clock the stock 491.52 MHz;
the stock system logged no underrun in seven idle minutes (`dmesg`, read-only). Open: when the
underflows come (their count per second next to the GPU's and the CPU copies' traffic).
