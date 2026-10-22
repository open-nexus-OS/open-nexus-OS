<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# 2026-10-12 — the display's frame clock and flip on the stock system (Block 4 layer 2, P0)

## Question

Before the scanout contract (RFC-0101 §8) is built on the board's controller: what is the
vertical blank as the stock system sees it — its period, its interrupt, the register that
raises it and the one that clears it — and does the stock system switch the scanned plane at
the blank, so our ring of planes flips the same way?

## Instrument

Read-only over adb as root on the stock system (vendor kernel 6.6.63), the HDMI desktop at
its login screen; nothing written to the controller or the device's storage. Two programs,
archived here, run from the volatile `/tmp` and removed:

- `vblank.py` — (a) the vertical blank's period through the kernel's own vblank wait on the
  KMS device (no master needed), 240 blanks; (b) the controller interrupt's rate from
  `/proc/interrupts` over five seconds; (c) a 45 000-samples-a-second read of the desktop
  channel's plane address, the output's line counter, the raw interrupt word and the
  config-ready latch for three seconds; (d) the interrupt, control, channel and output words.
- `irqwords.py` — four seconds of the whole interrupt block (`0x900..0x9bc`), recording which
  words change, to what, and at which scan line.

The names of the words are the ones the live dump of 2026-09-29 carries
(`../2026-09-29-display-regs/dpu-regs-on-annotated.txt`); the values are measured now.

## Results

**The frame clock.** 240 vertical blanks: median 16 667 µs, maximum 16 669 µs, one short
interval of 14 570 µs (a sequence counter 6851..7090, no gap). The mode is 1920x1080 at
60 Hz with the blanking the mode line names: 1125 lines, of which 45 are blank (front porch
2, sync 5, back porch 38) — the blank lasts about 667 µs.

**The interrupt.** The controller's online interrupt (the first of the two lines the tree
names) fires **120 times a second — twice a frame**. The pipeline's raw word `0x960` carries
three sources: bit 0 (vsync), set at line 1080, the first line of the blank; bits 2 and 3
(frame-timing end-of-configuration and configuration-line), set together at line 1. Its mask
word `0x910` reads `0xf212`: bits 1, 4, 9 and 12–15 masked, so exactly those three sources
are live — the two at line 1 arrive as one interrupt, the vsync as the other. The stock
driver clears the word by writing it (the raw word fell to 0 at line 1071 and rose again at
1078–1080); no other word of the block moves per frame (`0x938` changed once in four
seconds, to `0x6002`). The output control's own raw word `0x180b0` holds the same three
events (start-of-frame, end-of-configuration, end-of-line) with its mask `0x180b8 = 0x252`.

**The line counter** (`0x180c8`) runs 0..1080 and is readable at any time; it is the witness
of "inside the blank" for a flip.

**The flip.** No flip was observed: the desktop channel's plane address stayed at one value
for three seconds (the login screen is idle; one framebuffer is live, the pointer is the
second plane). The config-ready latch `0x56c` read 0 throughout. Whether a plane-address
write takes effect at once or at the next blank is therefore **not measured here** — the
first board cycle measures it on our own path (H2 below).

## Hypotheses the first board cycle decides

- **H1 — one interrupt a frame.** With `0x910` masking every source but bit 0 and the
  handler clearing bit 0 by writing `0x960`, our driver sees exactly 60 interrupts a second.
  *Decides:* `gpud: dc vblank ok (hz=60 …)` counted over the first second after the attach; a
  storm (>70/s) or silence (0) says the mask or the clear is other than read here.
- **H2 — the handler runs inside the blank.** The line counter at the handler's first read is
  ≥ 1080 or < 2 (the 45 blank lines). *Decides:* the `line=` values the marker prints for the
  first interrupts. If the handler is late (a present held the thread), the flip is deferred
  to the next blank — the policy, not a failure; the marker counts `late=`.
- **H3 — a plane-address write inside the blank shows no tear.** The address and stride of
  the other plane written in the handler, then the config-ready latch (the reveal's proven
  sequence): the operator sees windows move without a tear line and shadows that stay put.
  *Decides:* the operator's rung `board-visual: desktop` on this boot with a dragged window,
  plus `front=0` in every `gpud: dc present us` window (the tear gate).
- **H4 — the first present per frame costs what it cost.** The ring changes where a frame is
  drawn, not how: `gpud: dc present us` stays at the layer-0 numbers (median 150 ms) until G4
  — and the keystroke flicker is gone because nothing is drawn in view.

## Results of the board cycle (2026-10-12, image `dev-78566ebe`, `board--2026-10-12T13-06-02`)

The lines are archived in `board-2026-10-12T13-06-02-lines.txt`, the judge's transcript in
`judge-1306.txt`: **`[PASS] board-visible: 49 rungs, 7 operator marker(s), no FAIL marker`** —
the first full pass of the visible ladder since 2026-10-06, with the three new rungs.

- `gpud: dc vblank armed (irq=139)`; `gpud: dc flip ok (slot=1 line=1080)` — the first flip was
  written at line 1080, the first line of the blank.
- `gpud: dc vblank ok (min_period_us=21 line=331 late=0 n=32)`.
- Eighteen present windows, every one `front=0`; averages 72–542 ms (the layer-0 class: 97 %
  layer + blur), cache clean 0.8–1.5 ms, the first present 23.4 ms (the base copy included).
- `windowd: cursor upload failed`: 0 (8 on the layer-0 boot). Frames: content 19, re-blit 65,
  pointer 2, keep-alive 2, shape 0 of 86 presents.
- The operator: no tearing, no flicker, the pointer whole at the right border, every rung.

## Verdict

| | Hypothesis | Verdict |
|---|---|---|
| H1 | one interrupt a frame with the mask and the write-to-clear as read | **half.** The flip landed at line 1080 — the vsync source is live at the blank and the mask holds the two line-1 sources back. But the shortest interval between two served interrupts was 21 µs, and the first served interrupt came at line 331 (not a blank): a second interrupt follows a served one at once. Two readings compete — the raw word is not cleared by writing it (the stock driver's write took it to 0, ours may need the full word or another register), or a status of another block keeps the line raised. It costs one wake per flip today (the blank is disabled once the flip landed, so nothing storms) and `late=0` says it never pushed a flip out of the blank. **Next cycle's instrument** (TASK-0216 closure, owner: layer 3's first cycle): count interrupts served with nothing pending separately, and read the raw word back after the clear write. |
| H2 | the handler runs inside the blank | **holds for the flips** (`line=1080`, `late=0` over the witness window). The first served interrupt at line 331 is H1's second interrupt, not a late handler. |
| H3 | a plane address written inside the blank shows no tear | **holds** — operator: no tear line, shadows stable, the pointer whole; `front=0` in every window (the tear gate green). |
| H4 | the ring changes where a frame is drawn, not how much it costs | **holds** — 72–542 ms a present, the layer-0 class; the copy of the retained base adds about 3 ms (first present 23 ms against 8–16 ms before). The frame time is G4's. |

What follows: Phase 1 of RFC-0101 is on the board. The scanout contract's gates stay in the
ladder; the one open thread (the second interrupt) is a measurement for the next board cycle,
not a correctness fault today.

## Correction (2026-10-22, TASK-0342)

The interrupt words were read backwards above. The vendor kernel's register header (reference
only) names `0x910` an ENABLE word (`onl2_nml_*_int_msk`: a set bit lets the source raise the
line), `0x938` the latched status (`*_int_sts`, cleared by writing its bits — the word that
"changed once to `0x6002`": the stock handler clears it within microseconds) and `0x960` the raw
word (read-only). The stock `0xf212` therefore turns ON eof, cfg-ready-clear, underflow and the
command list's first four channels, not "vsync, cfg-eof and cfg-line live". H1's "second
interrupt at once" was this: our "mask" turned sources on and our clear wrote the read-only raw
word, so the line never fell. Board cycles 59–60 measured the consequence
(`../2026-10-22-frame-chain/README.md`).
