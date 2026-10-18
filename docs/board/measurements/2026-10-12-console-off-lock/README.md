<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# 2026-10-12 — the console off the kernel lock (TASK-0330 S2a): the gate shown red, then the fix

## Question

Does a transmit queue drained by the UART's own interrupt take the console's serial time off
the kernel lock on the board — and does the gate `KSELFTEST: console off lock ok` see it when
it does not?

## Instrument

The boot-end record of the steady window: `KINIT: bkl hold histogram … top nr=…` (the three
longest holders by syscall; 16 = `debug_putc`, 44 = `debug_write`), `KSELFTEST: console off
lock ok|FAIL (max_us= budget_us=1000)`, and — from the second cut — `KINIT: console tx
(irq_bursts= full= lock_fallback= max_depth=)`: bursts the interrupt sent, pushes that found the
queue full, pushes that found its lock held past the bound, the deepest the queue got.

## Results

**First cut, board `board--2026-10-12T14-54-22`** (`board-…-first-cut-lines.txt`,
`judge-first-cut-red.txt`): the queue was live (`KINIT: console tx queue (irq=42
fifo_burst=16)`), and the gate was **red** — `KSELFTEST: console off lock FAIL max_us=9571`,
`top nr=16:9571us nr=44:5463us`; the bring-up's holds unchanged (10–22 ms, `nr=44`). The lane
failed on it: `[FAIL] board-visible: FAIL markers in the trace` — the gate's first red on a real
failure, as `gates.md` asks of a new gate.

**Reading.** 9.6 ms for a ONE-byte syscall (`debug_putc`) is not a line's serial time; it is a
wait for the FIFO. The first cut enabled the FIFO (a burst per interrupt) but kept the polled
fallback byte-wise: with the FIFO on, "transmitter empty" means the whole FIFO, so every polled
byte waited for the FIFO's drain (64 bytes at 87 µs — 5.6 ms), and a push that found the queue
full drained sixteen bytes that way under the queue's lock with interrupts off; a pusher on
another hart then waited past its bound and wrote straight — another FIFO wait. Whether the
interrupt drained at all was not visible: the first cut had no counter.

**Second cut** (this commit): the polled fallback waits once for the empty FIFO and writes a
burst; the queue's counters are printed in the boot-end record.

QEMU, one hart (`just ci-os-smp1`, `smp1--2026-10-12T14-56-53`): `KINIT: console tx (irq_bursts=5371
full=0 lock_fallback=0 max_depth=1670)`, `KSELFTEST: console off lock ok (max_us=42 budget_us=1000)`
— the interrupt drains every byte there, the queue never fills, the trace contract stays
byte-equal.

**What the queue exposed on QEMU** (`usb-visible` lane, 2026-10-12 afternoon): with the queue on,
the lane went red three times in a row at three different places — `screencapd: deny
(reason=storage)` after `virtio-blk: timeout`, a shutter press that produced no save, a clipboard
text commit that never arrived — and green with the queue off. Bisected with the queue's
interrupt off (red) and the queue bounded to 64 bytes (red): the fault was not the interrupt
and not the output lag but the services running unthrottled. The red logs carried `WINDOWD: FAIL
app event send` and `WINDOWD: FAIL desktop text send`: windowd sent user intent (a text commit, a
key, a tap's ack, the shutter) to its clients with a one-shot non-blocking send and DROPPED the
frame when the client's queue was full — the eight-millisecond console stall after every line
had throttled it below the queue's depth. The frames now take the parking delivery
(`Delivery::Critical`: the kernel wakes the sender the instant the client drains), like the
taps; with it the lane is green with the full queue (`usb-visible--2026-10-12T17-04-52`:
`console tx (irq_bursts=5868 full=0 max_depth=1675)`, `console off lock ok (max_us=77)`,
clipboard and screenshot green, no block timeout).

**Second cut on the board, `board--2026-10-12T18-46-45`**: the queue live, every rung acked,
and the gate **red** again — `console off lock FAIL max_us=16444`, `bkl budget FAIL
max_hold=16ms nr=44`, and the counter said why: `console tx (irq_bursts=0 full=3885
lock_fallback=0 max_depth=8192)` — the UART's interrupt never fired on the board (QEMU: 5 900
bursts, never full). The queue filled at once and every push past it sent a burst polled.

**Reading, from the stock system (read-only, `adb`).** The console is `serial@d4017000`
(`/chosen/stdout-path` = `serial0`), `interrupts = <42>`, `reg-shift = <2>`, with `dmas` for rx
and tx — a PXA-style 16550. Live registers under the vendor driver: `IER = 0xC0` (bit 6 the
unit enable, **bit 7 DMA requests**), `MCR = 0x0B` (**OUT2** set), `IIR = 0xC1`, `LSR = 0x60`;
and `/proc/interrupts`: PLIC 42 **counts zero** — the vendor console transmits by DMA, its
transmitter interrupt was never exercised on this board by anyone. Two hypotheses with one
decision line each: with the DMA bit set the empty transmitter raises a DMA request instead of
the interrupt (the second cut kept IER's bits 4–7 as the firmware left them); the 16550 family
gates its interrupt output on MCR OUT2, which the boot loader leaves clear.

**Third cut** (this commit): at enable, IER keeps only bits 5–6 (the DMA-requests bit and the
receive-timeout bit cleared with the kernel's own four), MCR gets OUT2, and the boot line reads
the registers back: `KINIT: console tx queue (irq= fifo_burst= ier= mcr= iir= plic_enabled=)`.
Two more changes make the queue independent of the line's trigger mode (the stock tree lists
PLIC 42 as edge): a pusher that finds the transmitter idle fills it itself (no wait: the FIFO is
empty; counted as `kicks=`), and the interrupt's drain waits briefly for the queue's lock
instead of returning (an edge missed while a pusher held the lock would otherwise never
return). QEMU (`headless--2026-10-12T19-45-27`): `ier=0x00 mcr=0x08 iir=0xc1 plic_enabled=true`,
`console off lock ok (max_us=56)`, `console tx (irq_bursts=1322 kicks=56663 full=0
max_depth=1)` — the emulator's transmitter is idle at nearly every push, so the pusher sends.

**Board, `board--2026-10-13T08-28-06`** (image `dev-9f252dd5`, all seven rungs acked,
clipboard and screenshot included):

| line | value |
|---|---|
| `KINIT: console tx queue` | `irq=42 fifo_burst=16 ier=0x40 mcr=0x08 iir=0xc1 plic_enabled=true` |
| `KSELFTEST: console off lock` | **ok** `max_us=273 budget_us=1000` |
| `KINIT: console tx` | `irq_bursts=2348 kicks=962 full=1059 lock_fallback=0 max_depth=8192` |
| `KSELFTEST: bkl budget` | **ok** `max_wait=3050us max_hold=0ms nr=20` (second cut: FAIL 16 615 µs / 16 ms) |
| `KINIT: bkl hold histogram` | `le1ms=1723 le10ms=0 gt10ms=0 top nr=20:337us nr=44:273us nr=16:203us` (second cut: `le10ms=311 gt10ms=4 top nr=44:16444us nr=16:9229us`) |
| judge | `[PASS] board-visible: 50 rungs, 7 operator marker(s), no FAIL marker` |

The interrupt fires (2 348 bursts), the console syscalls are off the lock (273 µs against
16 444), and no hold in the steady window is above a millisecond — the first board boot whose
kernel-lock budget is green without an allow-list line (the entry `KSELFTEST: bkl budget FAIL`
is removed from `config/fail-marker-allow-board.txt`). `full=1059` is the bring-up's burst
overflowing the 8 KiB queue (expected; each costs one polled FIFO wait, inside the 273 µs).

**Two findings handed on (not the console's):**

- `KSELFTEST: ipc call budget FAIL (rt=97us budget=64)` — new in this cycle (56 and 60 µs in
  the two cycles before; QEMU unchanged at 43). The bench is the mean of 64 round trips to
  samgrd (TASK-0054C P6), the budget calibrated on the emulator; the two printed benches doubled
  with it (`ipc bench` 58→121, `ipc call bench` 55→107), the kernel's IPC path is untouched by
  this package. Once the console left the lock the boot compressed and the hart placement
  shifted (`KINIT: ipc sends by hart` `[2511, 3649, 153, 1631]` → `[1781, 678, 4627, 33]`): a
  cross-hart wake costs the IPI, and samgrd answers the fleet's registrations at bench time.
  Owner: **S3** (placement, IPI cost; a same-hart bench or the board's own budget). Tolerated on
  the board with that reference (`config/fail-marker-allow-board.txt`).
- Operator, this cycle: typing after maximizing a window is barely possible (the cycle before:
  nothing worked after it). The input path is healthy throughout (`xhcid: irq hz≈95 … dropped=0`,
  `hidrawd … tx hz≈96`); app-host's frame arena spills on the maximized layout (`frame arena
  (… peak=519784 of=1048576 spill=1)`, `SELFTEST: frame arena spill FAIL` ×11) — a frame larger
  than its generation on every key. Owner: the **memory lane** (TASK-0290 "Input from Block 1":
  M2 demand paging + M5 purgeable generations); no interim bump (operator 2026-10-12: nothing
  changed that the memory lane replaces).

## Verdict

- The console's serial time is off the kernel lock on the board: 273 µs against 16 444 for the
  longest console hold, the kernel-lock budget green for the first time on a board boot, the
  steady window without a hold above a millisecond.
- The board's UART needed two register facts no emulator shows: IER's DMA-requests bit must be
  clear (the vendor console transmits by DMA and its interrupt line had never fired on this
  board) and MCR OUT2 must be set. Both are read back in the boot line, so the next board with
  a different UART decides from its trace.
- The gate `KSELFTEST: console off lock ok` is a real gate: it was red on the board on a real
  fault (the first cut's byte-wise wait) before it was green.
- The slow console had been a throttle that masked a dropped-frame bug in windowd's client
  delivery; the queue surfaced it and it is fixed at the root (no frame of user intent is ever
  dropped). The three `virtio-blk: timeout` lines of the first red run did not recur in the
  green runs; they stay a watched class (`manual--2026-10-10` saw one with the synchronous
  console).

## S2b — the serial line reads the ring (TASK-0330 S2b, 2026-10-18)

**Question.** S2a's queue held 8 KiB; a write into a full queue sent its oldest bytes polled
first. Does that still hold harts during a board bring-up, and can the serial line read the
console ring itself, so that no writer ever waits for it?

**Measured first** (board cycles 25 and 26 of the GPU lane, `board--2026-10-17T15-48-51`,
`board--2026-10-17T21-48-46`; the code; QEMU):

- Every bring-up filled the queue: `KINIT: console tx (… full=1175 … max_depth=8192)` and
  `full=1139`. Each full push waited for one FIFO burst on the line (16 bytes, 1.4 ms at
  115 200 baud) in the trap, interrupts off: about 1.6 s of hart time a boot (an estimate from
  the counter, not a measurement of the waits).
- The bring-up's longest kernel-lock holds were console writes: `KINIT: bkl bring-up burst
  max_wait=28868us max_hold=20ms nr=44 gt10ms=41` (cycle 25: `28658us`, `21ms`, `nr=44`,
  `38`). `debug_write` (44) runs under the kernel lock (it went back to the lock class so the log
  channel stays serialized), so a line written into a full queue held the lock for its serial
  time, and every other hart's syscall waited.
- The GPU lane's cycle 26: the containment rung's lost context took 130 ms to tear down
  (`f_teardown_us=129833`; cycle 25: 1450) while the supervision probe's second crash report —
  1540 bytes, 133.7 ms of serial time — lay between the rung's loss line and its verdict. A user
  fault's report is written in the trap: its hart takes no timer meanwhile, and a task homed on
  it waits unless an idle hart steals it. Which hart gpud was on is not measured.
- Nobody reads the board's serial line: the proof is the ring the block owner keeps on the boot
  disk (RFC-0107). QEMU's transmitter never fills the queue (`full=0 max_depth=1`).

**What S2b builds.** The serial line reads the ring from a cursor of its own
(`console_tx::Cursor`, host-tested); a write records its byte and kicks the line — an idle
transmitter is filled at once, otherwise the transmitter-empty interrupt is live while the line
owes bytes. No writer waits for the line; the 8 KiB queue is gone (the queue is the ring's span
between the cursor and the head). A line the writers lap — more than the ring, less a 4 KiB
margin, behind — skips to the oldest byte kept and says so in one line; a panic sends what the
line owes, polled, before its last words; the timer tick backs up a kick that found the line's
lock held. New lines: `KINIT: console tx (irq_bursts= kicks= behind_max= lapped=
lock_fallback=)` and `KINIT: user fault reports (n= max_us= total_us=)` (how long a user fault's
report held its hart).

| # | Hypothesis | Decision line |
|---|---|---|
| H1 | The bring-up's long kernel-lock holds were console writes waiting for the line | `KINIT: bkl bring-up burst …`: `max_hold` no longer names `nr=44`, `gt10ms` falls from 41 to a few at most |
| H2 | The ring absorbs the bring-up's burst without a loss | `KINIT: console tx (… behind_max= lapped=0 …)`: `behind_max` above 8192 (more than the old queue held), `lapped=0` |
| H3 | A crash report no longer holds its hart for its serial time | `KINIT: user fault reports (n= max_us=)`: `max_us` below 10 000 (the 1540-byte report's serial time is 133 700) |
| H4 | The console's steady hold stays as it was | `KSELFTEST: console off lock ok (max_us=)` near cycles 25–26 (202, 269) |
| H5 | The GPU rung's teardown no longer depends on a crash report | `gpu containment ok (… f_teardown_us=)` near cycle 25's 1450, whether a report falls into the rung or not |

**QEMU** (2026-10-18): `headless` and `visible` green, the trace contract byte-equal (65 262 and
66 987 bytes of kernel console text on the disk, equal to the UART), `console tx (irq_bursts=0
kicks=56280 behind_max=1 lapped=0 lock_fallback=0)` — QEMU's transmitter takes every byte at
once — and `user fault reports (n=6 max_us=980 total_us=5882)`. The two-hart `smp` lane is red as
before for its own reasons (`ipc call budget FAIL rt=210us` as on 2026-10-16; `settingsd: event
send FAIL … QueueFull` seen there before too), no new FAIL, the trace contract byte-equal,
`console off lock ok (max_us=871)` (754 and 1044 on 2026-10-16), `user fault reports (n=6
max_us=10550)` under the emulator's two threads.

**Results — the board, cycle 27** (two boots of one image, `board--2026-10-18T14-50-09` and
`board--2026-10-18T15-01-29`; `s2b-board-lines.txt` with cycle 26 beside them;
`s2b-board-test.txt`: `[PASS] board-visible`, 66 rungs, the operator's seven confirmations on the
second boot):

| | Cycle 26 (S2a) | Cycle 27, boot 1 | Cycle 27, boot 2 |
|---|---|---|---|
| bring-up lock: longest hold, holds over 10 ms, longest wait | 20 ms (`nr=44`), 41, 28 868 µs | under 1 ms (`nr=6`), 0, 1 499 µs | under 1 ms (`nr=6`), 0, 1 475 µs |
| the line: farthest behind, skipped | the queue full (`full=1139`, 8192) | 22 491 bytes, 0 | 22 249 bytes, 0 |
| user fault reports: longest, all six | about 134 ms by the serial time | 3 030 µs, 13 591 µs | 3 126 µs, 13 649 µs |
| console off lock | 269 µs | 249 µs | 195 µs |
| the GPU containment rung's teardown | 129 833 µs (a report inside the rung) | 1 504 µs | 1 501 µs |
| windowd's GPU context open (TASK-0215 P3) | 9 971 µs | 6 119 µs | 6 098 µs |
| `ipc call budget` | FAIL (76 µs) | ok (44 µs) | ok (44 µs) |

- **H1 confirmed.** The bring-up's long holds were console writes: no hold above 10 ms in either
  boot (41 before), the longest under a millisecond and no longer `nr=44`, the longest wait for
  the lock 1.5 ms (28.9 before).
- **H2 confirmed.** The line fell up to 22.5 KB behind — 2.7 times what the old queue held — and
  caught up without skipping a byte (`lapped=0`, no `[serial:` line).
- **H3 confirmed.** A crash report held its hart 3.0–3.1 ms at most; the 1540-byte report's
  serial time is 133.7 ms.
- **H4 confirmed.** The steady console hold 195–249 µs (202–269 before).
- **H5 not decided by an overlap.** In both boots the probe's reports fell before and after the
  containment rung; the teardown took 1.5 ms as in cycle 25, and H3 bounds what a report inside
  it could now add: about 3 ms, not 128.
- **Two boots, the same numbers** within a few percent (H1–H4 and the GPU rungs).
- **Side findings.** windowd's GPU context opened in 6.1 ms in both boots against 10.0 ms in
  cycle 26: the 3.4 ms that the GPU lane's H65 put on the open's own steps were the bring-up's
  lock waits, and H65's prediction (near the rung's binding, `bind_us=6392` this cycle) holds —
  corrected in `docs/board/measurements/2026-10-13-gpu-render-queue/README.md`. The boot:
  `init: timing total_ms=501` against 566–567, hidrawd ready after 1.66 s against 2.0–2.2 s —
  one cycle, observed, not a gate. `KSELFTEST: ipc call budget` ok in both boots (44 µs); it
  failed in 12 of 48 board boots before, which one cycle does not decide.
- **Boot 1's screenshot hung** after the selection was dragged (the operator rarely drags it):
  windowd stopped taking input — inputd's pushes stopped, hidrawd's sends failed with
  backpressure until the reset — with no `capture freeze off` and no save. The same signature
  appeared on 2026-10-14 (`board--2026-10-14T11-25-48`), before S2b; boot 2 dragged the
  selection and saved. Not the console's: handed to TASK-0215 P4, whose cycles drag the
  selection on the new present path.

**Verdict.** No console write waits for the serial line any more: the board's bring-up holds the
kernel lock for no line's serial time, a crash report holds its hart for milliseconds, and the
serial line, reading the ring, catches up without losing a byte. S2b complete.
