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
