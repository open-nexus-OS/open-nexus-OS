<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# 2026-10-10 — the baseline for memory, GPU and SMP (Block 4, layer 0)

Status: **instrument built, hypotheses written, the board boot is outstanding.** Results and
verdict are filled from that boot; nothing below the "Results" heading is a board number yet.

## Question

Before any of the three lanes changes code: what does a frame cost on the board and where does
the time go, who holds the kernel lock and for how long, and whose memory is resident? And one
question about the gates themselves: do the lanes see what the board prints?

## Instrument

All of it is in the tree and prints windows or one-time records, never a line per event.

| Line | From | Says |
|---|---|---|
| `gpud: dc present us (n=32 avg= max= clean= blur= layer= shadow= blit= fill= other= damage_px= front=)` | `userspace/nexus-gfx/src/backend/dc/frame_stats.rs`, fed by `source/drivers/gpud/src/backend/dc/present.rs` | a 32-present window on the controller path: average and slowest present, the cache clean, the time by command kind, the damage, and `front` — presents drawn into the plane the controller was scanning |
| `windowd: loop hz=… frames(content= blit= pointer= keepalive= shape=)` | `source/services/windowd/src/compositor/frame_causes.rs` | why the window's frames were sent: content recomposited, a retained region re-blitted, the pointer's region alone; and two causes of pointer-region frames without a move — the frame clock's keep-alive and a pointer shape change |
| `KINIT: bkl hold histogram le100us= le1ms= le10ms= gt10ms= top nr=A:us nr=B:us nr=C:us` | `source/kernel/neuron/src/core/trap/lock_stats.rs` | how long ecalls held the kernel lock in the steady window, and the three longest holders by syscall number |
| `KINIT: bkl histogram …`, `KSELFTEST: bkl budget …` | same | the wait side — now in microseconds on every timebase, and for the steady window only |
| `KINIT: ipc sends by hart [..]` | `source/kernel/neuron/src/ipc/stats.rs` | the window's sends by the hart that made them |
| `KINIT: mm space pid= sid= rss= dma=` (12 lines) | `source/kernel/neuron/src/syscall/api/sched_telemetry.rs` | the twelve largest address spaces at the boot-end fence; `spaces.py <uart.log>` names them |

Two faults of the old instrument, found while reading it and fixed with the new one:

1. **Units.** The wait histogram's bounds and three log lines were literal ticks at QEMU's
   10 per µs. The board's timebase is 24 per µs, so the board's "gt10ms" bucket counted every
   wait above 4.2 ms, and `KINIT: bkl wait …us` read 2.4 times too high. The gate's `max_wait`
   and `max_hold` were converted correctly and stand.
2. **The window.** The call that prints the bring-up lines also reset the accounting — and was
   then recorded, with every hart queued behind it, as the first entry of the fresh window. A
   sample now belongs to the window it began in (host test
   `lock_stats::tests::a_sample_counts_for_the_window_it_began_in`).

And one of the gate: the FAIL gate of both harnesses anchored on the bare marker, the kernel
prints behind its logger prefix (`[ERROR smp] KSELFTEST: bkl budget FAIL …`). Every archived
board trace that carries the verdict says FAIL; none was judged for it. The gate reads the
prefix now; the transcripts of the judge on a copy of `board--2026-10-10T12-15-36` are the
proof (before: green through the gate; with the new expression: red on that line; with the
tracked entry in `config/fail-marker-allow-board.txt`: tolerated, counted).

## Hypotheses, each with what decides it

**H1 — the board's `max_hold=22ms nr=48` is the instrument.** Syscall 48 is the scheduler
call whose telemetry op prints three lines (about 250 characters) and resets. 22 ms over 250
characters is 88 µs a character — a serial line at 115200 baud, written under the lock.
*Decides:* the first board boot with the window rule. If `max_hold` drops to a few
milliseconds and names another syscall, H1 holds and the steady state's real worst holder is
on the `top` line. If it stays near 22 ms with another number, the hold is real and S2 starts
there. Either way the finding behind it stands: a kernel log line under the lock costs its
length in serial time on this board.

**H2 — the wait maximum (25 ms) is the same event.** *Decides:* same boot; `max_wait` falls
with `max_hold`, or it does not and the hold histogram's `gt10ms` names how many real ones
there are.

**H3 — a desktop present costs 100 ms because of the glass layers, not the bus.** The first
present (a full plane, no glass) costs 14–16 ms; the steady average is 100 ms. *Decides:* the
`layer` and `blur` columns carry most of `avg`; `blit` + `fill` + `clean` stay near the first
present's cost.

**H4 — every present after the reveal is drawn into the scanned plane.** *Decides:*
`front` equals `n` in every window after `gpud: dc reveal flip ok`. This is the tear gate's
red state; the scanout contract turns it to `front=0`.

**H5 — a share of the frames need not exist.** *Decides:* `keepalive` + `shape` as a share of
the window's presents while hovering and typing. A double-digit share is the first cut of the
scanout contract's package; a share near zero says the frames are real work.

**H6 — the IPC load sits on few harts.** *Decides:* `ipc sends by hart` — the split order of
the lock (ADR seed, S2/S3) follows the distribution, not the intuition.

**H7 — one address space dominates resident memory.** On QEMU (one hart, 1280x800 layout
block) gpud holds 96 MiB of 157 MiB. *Decides:* the board's table; the memory lane's first
package is cut from the top rows.

## Results

QEMU, one hart (`smp1--2026-10-10T18-33-28`, lane green) — the instrument's own proof, not a
baseline for the board:

- `KINIT: bkl hold histogram le100us=650692 le1ms=2355 le10ms=0 gt10ms=0 top nr=11:166us nr=20:160us nr=46:160us`
- `KSELFTEST: bkl budget ok (max_wait=0us max_hold=0ms nr=11)`
- resident: gpud 96.05 MiB, an app-host child 11.56, windowd 10.61, init 10.31, execd 7.03;
  `rss_sum` 156.6 MiB over 40 spaces.

Board: outstanding.

## Verdict

Outstanding. It names, from the numbers: the board's own lock budget (the constants in
`core/trap/budgets.rs` are calibrated for the emulator), the frame-time gate's first
threshold, the tear gate (`front=0`), and the order of the first packages of each lane.

## Not measured here

- Input to photon. It needs a present that can be attributed to the input that caused it;
  that identity arrives with the frame clock and the fence of the scanout contract and is
  measured in that package's P0.
- `-smp 8`. The kernel runs four harts until S1 (`MAX_CPUS`); the eight-hart series belongs
  to S1's gate.
- The warm-reset hang (`../2026-10-09-smp-start-hang/`): its own series, warm against cold.
- The GPU's hardware truth and the stock system's frame time: their own folders.
