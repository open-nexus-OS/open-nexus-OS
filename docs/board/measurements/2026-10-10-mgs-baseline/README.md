<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# 2026-10-10 — the baseline for memory, GPU and SMP (Block 4, layer 0)

Status: **measured.** The hypotheses below were written before the board boot; the results and
the verdict are from one boot of the same day.

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
tracked entry in `config/fail-marker-allow-board.txt`: tolerated, counted). The transcripts are
`judge-before.txt`, `judge-after-red.txt` and `judge-after-allow.txt`; `judge-2044-acked.txt` is
the judge on this package's own boot (ladder 46 of 46, six operator rungs confirmed, the
clipboard rung not shown on this boot — the lane says FAIL for it, and nothing was acked
after the fact).

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

Board, one boot of image `dev-b4f5` (`board--2026-10-10T20-44-49`, four harts, 1920x1080; the
operator hovered, typed, dragged, tiled, opened the chat and took a screenshot). The lines are
archived in `board-2026-10-10T20-44-49-lines.txt`; the ladder is complete (46 rungs) and the
FAIL gate clean on this trace.

**The kernel lock, steady window**

- `KSELFTEST: bkl budget ok (max_wait=10615us max_hold=10ms nr=16)` — the first `ok` this
  verdict ever printed on the board.
- `KINIT: bkl hold histogram le100us=1645880 le1ms=103 le10ms=146 gt10ms=1 top nr=16:10540us nr=44:5043us nr=58:2288us`
- `KINIT: bkl histogram le100us=1665820 le1ms=2467 le10ms=372 gt10ms=3` (waits)
- Syscalls 16 and 44 are the two console writes; 58 is the IPC call.
- Bring-up window: `max_wait=48136us max_hold=22ms nr=44`, and seven `KINIT: long ecall
  nr=44` of 10–19 ms.
- `KINIT: ipc sends by hart [80, 1, 1969, 2761, …]` — 4811 sends, 98 % from harts 2 and 3.

**A present on the controller path** (16 windows of 32 presents)

| | |
|---|---|
| window averages | 40 ms … 1008 ms, median 151 ms; slowest single present 1534 ms |
| share `layer` (composited layers with their own shadow and backdrop blur) | 70.5 % |
| share `blur` (backdrop blur on its own) | 26.5 % |
| share `blit` | 2.4 % |
| share cache clean | 0.5 % (0.8–3.7 ms a present) |
| damage | the full plane (2 073 600 px) in every window after the first |
| `front` | 32 of 32 in every window after the reveal (28 of 32 in the window that holds it) |
| first present (no glass) | `exec_us=8207 clean_us=750` |

**Why frames were sent** (the 20 loop windows with eight presents or more, 239 presents):
content 69, re-blit of a retained region 163, pointer region alone 62, keep-alive 4, pointer
shape 0. `windowd: cursor upload failed` 8 times.

**Resident memory** (`rss_sum` 162.9 MiB over 40 spaces): gpud 103.86 MiB, of which 102.85 MiB
is the one contiguous layout block; an app-host child 11.56; windowd 10.61; init 10.32; execd
7.03; every other space below 1.5 MiB. Kernel heap in use 1.1 MiB. The chat's frame spilled
the arena once (`spill=1`, peak 454 857 of the 1 MiB arena).

## Verdict

| | Hypothesis | Verdict |
|---|---|---|
| H1 | the 22 ms hold was the instrument | **holds.** With the window rule the steady maximum is 10.5 ms, and it is a console write |
| H2 | the 25 ms wait was the same event | **holds.** 10.6 ms, three waits above 10 ms |
| H3 | the glass layers cost the present, not the bus | **holds.** 97 % is layer + blur; copying and cleaning the plane is 3 % |
| H4 | every present is drawn into the scanned plane | **holds.** `front` = `n` |
| H5 | a share of the frames need not exist | **refuted.** Keep-alive and shape are 4 of 239. What dominates is the re-blit frame of an animation tick (163) — real frames, each paid as a full composite |
| H6 | the IPC load sits on few harts | **holds.** Harts 2 and 3 send 98 %; hart 0 (the soft-real-time hart) 80 sends, hart 1 one |
| H7 | one address space dominates | **holds.** gpud's layout block is 63 % of all resident memory |

What follows for the lanes:

1. **The worst lock holder left in the steady state is the console.** Every one of the three
   longest holders but the IPC call is a console write, and the bring-up window's 16–19 ms
   holds are console writes too: a line written under the lock costs its length in serial
   time. That is the first package of S2, ahead of the scheduler split — and it is small.
2. **The lock budget's constants are the emulator's** (40 ms wait, four convoys, 10 ms hold).
   The board passes them at the edge (hold 10.5 ms against 10). The tracked entry in
   `config/fail-marker-allow-board.txt` stays until S2's first package, which gives the board
   its own, tighter budget from these numbers; one boot at the edge is not a green gate.
3. **The frame is the CPU composite.** 150 ms a present is seven frames of a 60 Hz display,
   all of it drawn in view. No copy or cache cost stands in the way of a ring of planes: the
   clean of a whole plane is under 4 ms. The scanout contract (layer 1) takes the drawing out
   of view; the frame TIME is the GPU lane's (G4 deletes this composite) — nothing in the CPU
   layer or blur path is worth tuning.
4. **An animation tick costs a full frame.** The 163 re-blit frames are hover springs and
   transitions; on this path each is a whole composite. With a ring and a flip they stop
   being visible half-drawn; they stop being slow with G4.
5. **Hart 1 is idle and hart 0 nearly so** while harts 2 and 3 carry the IPC. Placement (S5)
   has room before any lock is split; the split order (S2/S3) is written against this
   distribution.
6. **The memory lane's first number is the layout block**: 103 MiB contiguous for gpud, most
   of it atlas rows that are reserved, not used. M2's commit-on-touch applies to it as much as
   to app-host's frame arena.

Gates this baseline defines (each is red on this trace, by construction):

- tear gate: `front=0` in every `gpud: dc present us` window — layer 1.
- frame time on the controller path: stays a measurement until G4; the gate is then set
  against the stock system's number for the same scene.
- lock hold, board: no console write among the top holders, and a budget below the frame
  (first value from S2's first package).

## Seen at the monitor on this boot (operator, not instrumented)

- **Shadows and translucent backgrounds darken with every present.** The CPU composite blends
  each layer over the previous frame's result instead of over a restored base: outside the
  restored glass rectangles (the shadow's halo, translucent desktop pixels) the alpha
  accumulates frame by frame. Reading: `cpu_frame.rs` blends in place, `restore_halo_pad` is 0
  (`windowd/src/compositor/material_glass.rs`). Owner: layer 1 — a frame composed off-screen
  starts from its base by construction; no fix in the in-place path.
- **Near the right edge of the screen the pointer's sprite breaks apart** in a vertical strip.
  The pointer is the controller's 64x64 layer; the strip is where its rectangle crosses the
  display's right border. Owner: layer 1 (the pointer plane), with a host test of the layer's
  rectangle at every border before the board sees it.
- The flicker on hover is unchanged (`front` = `n`, as measured).

## Not measured here

- Input to photon. It needs a present that can be attributed to the input that caused it;
  that identity arrives with the frame clock and the fence of the scanout contract and is
  measured in that package's P0.
- `-smp 8`. The kernel runs four harts until S1 (`MAX_CPUS`); the eight-hart series belongs
  to S1's gate.
- The warm-reset hang (`../2026-10-09-smp-start-hang/`): its own series, warm against cold.
- The GPU's hardware truth and the stock system's frame time: their own folders.
