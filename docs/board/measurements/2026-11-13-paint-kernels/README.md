<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# 2026-11-13 — what the painter's vector kernels gain (TASK-0362 P4)

Status: **measured on the board's stock system and on our system
(`board--2026-11-13T10-38-20`, image `dev-1d8bead9`); equality proven under the user-mode
emulator at four register widths, on QEMU and on the board. H1, H2 and H4 hold; H3 holds as
stated and the instrument cannot say more below 64 pixels.**

## Question

The row painter's flat spans — a fill, a source-over blend of one colour — run a vector kernel
chosen at run time (`nexus-paint-kernels`, ADR-0099 rule 4); the build carries no vector flag.
P0 measured what the compiler's own vectorisation of the whole program gains on the board
(`fill` 3.2 → 0.5 ns per pixel, `cards` 46.9 → 21.3). Do two hand-written kernels reach that,
where is the length below which a vector form loses, and what does it change on our system?

## Instrument

`rasterbench` of the P0 measurement
(`../2026-11-12-float-and-vectors/rasterbench/`), its `hard` build (F and D, no vector flag),
started with the argument `kernels`: it chooses the vector forms and prints, before the P0
workloads,

- `rasterbench kernels equal spans=8192 digest=…` — the crate's proof (`proof::run`): every
  span by the kernels as chosen and by the scalar reference, the digest the one pinned from
  the reference on the host; a difference ends the program with exit code 4;
- `rasterbench kernel=fill|blend px=… scalar_ns=… vector_ns=…` — one span of that length, the
  fastest of 2 000 calls of each form (16 px is the shortest span the vector form takes).

Without the argument the same program is P0's `hard` build: the scalar reference.

On our system: `SELFTEST: paint kernels ok (`, `apphost: paint kernels (`, and
`apphost: hops motion paint_us= rows=` of the same apps before
(`board--2026-11-13T09-35-00`, the scalar painter on hard float) and after.

## Hypotheses (stated before the board run)

| # | Hypothesis | Decision criterion |
|---|---|---|
| H1 | The fill kernel reaches what the compiler's vectorisation reached. | `fill` ≤ 1 ns per pixel with kernels (3.2 scalar, 0.5 compiler-vectorised) → holds. Above → the row's cost is not the store; the split inside a row (TASK-0367 P0) names it. |
| H2 | The blend kernel carries the gain of the translucent scene. | `cards` ≤ 25 ns per pixel with kernels (46.9 scalar, 21.3 compiler-vectorised) → holds. Above → the compiler vectorised something else in that scene (the corner or the hairline path); found by the split, not guessed. |
| H3 | A vector form is no slower than the scalar one from sixteen pixels on. | `vector_ns ≤ scalar_ns` at `px=16` for both kernels → `SHORT` stays; slower → `SHORT` moves to the first length at which it wins, in the next image. |
| H4 | The picture does not change. | The seven painter digests equal P0's; `kernels equal` with the pinned digest. One difference → the kernel is wrong, nothing ships. |

## Equality (no timing): the user-mode emulator

`qemu-riscv64 -cpu max,vlen=N rasterbench-hard kernels`, N = 128, 256, 512, 1024 (registers of
16, 32, 64 and 128 bytes):

| Registers | `kernels equal` digest | The seven painter scene digests |
|---|---|---|
| 16 bytes | `287e0815b292fd61` | equal to P0's |
| 32 bytes | `287e0815b292fd61` | equal to P0's |
| 64 bytes | `287e0815b292fd61` | equal to P0's |
| 128 bytes | `287e0815b292fd61` | equal to P0's |

`287e0815b292fd61` is `proof::DIGEST`, what the scalar reference yields on the host. The
emulator's times are no measurement (it runs a vector instruction slower than a scalar one).

## The board's stock system (`board-hard-scalar.txt`, `board-hard-kernels.txt`)

CPU 0 at 1.6 GHz, the governor at `performance`. ns per pixel of the target; `scalar` is the
same program without the argument (the reference), P0's columns from
`../2026-11-12-float-and-vectors/README.md`:

| Scene | P0 scalar | P0 compiler-vectorised | scalar, the reference | kernels | reference / kernels | Digest |
|---|---|---|---|---|---|---|
| `fill` | 3.2 | 0.5 | 3.2 | 0.6 | 5.3× | `b1cbc9f3b8eaa325` = |
| `cards` | 46.9 | 21.3 | 53.5 | 15.5 | 3.5× | `4303b247540b2465` = |
| `shadow` | 157.5 | 152.5 | 159.8 | 156.8 | 1.02× | `33362f18d26ca595` = |
| `circles` | 64.4 | 61.7 | 65.2 | 63.3 | 1.03× | `c8f7de22ed47b385` = |
| `strokes` | 115.0 | 112.3 | 115.8 | 114.5 | 1.01× | `00bc0187353c8585` = |
| `filled` | 27.7 | 27.5 | 28.1 | 25.7 | 1.09× | `523bad3b1b26f94e` = |
| `page` | 226.1 | 222.5 | 239.2 | 226.9 | 1.05× | `d75351079323010b` = |

A second run of the kernels: `fill` 0.5, `cards` 15.2, `filled` 26.9, `page` 222.7.
`rasterbench kernels equal spans=8192 digest=287e0815b292fd61` on 32-byte registers.

One span, the fastest of 2 000 calls (ns for the call, two clock reads included):

| Pixels | fill scalar | fill vector | blend scalar | blend vector |
|---|---|---|---|---|
| 16 | 541 | 500 | 1 041 | 541 |
| 24 | 541 | 500 | 1 333 | 541 |
| 32 | 583 | 500 | 1 625 | 541 |
| 64 | 666 | 500 | 2 791 | 583 |
| 128 | 875 | 541 | 5 083 | 625 |
| 512 | 2 083 | 583 | 18 791 | 1 041 |
| 1920 | 6 500 | 875 | 69 208 | 2 541 |

The floor of this table is about 500 ns — the two clock reads around a call, on a clock that
ticks every 41.7 ns. Above it: a 1920-pixel fill costs 3.1 ns per pixel scalar and 0.2
vector; a 1920-pixel blend 35.8 scalar and 1.1 vector (the second run printed the same
numbers).

## Our system (`board--2026-11-13T10-38-20` against `board--2026-11-13T09-35-00`)

`SELFTEST: paint kernels ok (vector=1 width=32 spans=8192 digest=0x287e0815b292fd61)`;
`apphost: paint kernels (vector=1 width=32)` for each of the six app-hosts; the ladder 118
rungs, `KSELFTEST: bkl budget ok (max_wait=3595us max_hold=1ms)`.

`apphost: motion layer … paint_us`, the same layers, the scalar painter on hard float → the
kernels:

| Layer | before | after | |
|---|---|---|---|
| 343×122 (three in a row) | 3 756–3 773 | 3 022–3 103 | −19 % |
| 402×229, with a backdrop | 5 386 | 4 165 | −23 % |
| 391×548 | 45 992 | 38 054 | −17 % |
| 393×687 | 60 414 | 50 587 | −16 % |
| 351×566 | 27 734 | 23 652 | −15 % |
| 265×139 | 6 614 | 6 034 | −9 % |
| 402×240 | 18 506 | 17 232 | −7 % |

`apphost: ux launch render_ms`: 99–442 → 98–393; three of six launches inside the 300 ms
budget (two before).

## Verdict

| # | Verdict |
|---|---|
| H1 | **Holds.** `fill` 3.2 → 0.6 and 0.5 ns per pixel: the compiler's number without a vector flag in the build. |
| H2 | **Holds, and below the compiler's number.** `cards` 15.5 and 15.2 against 21.3 (why the compiler's loop is slower was not looked at). |
| H3 | **Holds as stated; the question behind it stays open below 64 pixels.** At 16 pixels the vector blend costs half the scalar one (541 against 1 041 ns, floor included) — its crossover lies below `SHORT`. The fill's two forms are one clock tick apart up to 32 pixels: this table cannot order them there. |
| H4 | **Holds.** Seven digests equal P0's, the proof's digest the host's, on 16, 32, 64 and 128-byte registers. |

## What the numbers say beyond the question

1. **The spans are the smaller part of a real layer's row.** The two span scenes gain 3.5×
   and 5.3×, a real motion layer 7–23 % — if its spans gained what the scenes' did, they were a
   tenth to a third of its time (an estimate, not a measurement). The rest is per-pixel work:
   glyphs, corners, shadows, the straight-alpha conversion every row leaves through. The split
   inside a row is TASK-0367 P0's measurement; the next kernels follow it.
2. **The scalar reference's blend is slower than the loop it replaced** (`cards` 53.5 against
   46.9 ns per pixel, `page` 239.2 against 226.1). With the vector unit it paints only spans
   under sixteen pixels; `page` with kernels is where it was (226.9 and 222.7 against 226.1).
   Recorded at TASK-0367 P4 together with the blend's crossover: lower `SHORT` for the blend,
   measured with calls timed in batches.
3. **Timing one call cannot resolve a short span on this clock.** A by-length table needs a
   batch of calls per clock read.

## Reproduce

```bash
docs/board/measurements/2026-11-12-float-and-vectors/rasterbench/build.sh /tmp/rasterbench
for v in 128 256 512 1024; do qemu-riscv64 -cpu max,vlen=$v /tmp/rasterbench/rasterbench-hard kernels; done
adb push /tmp/rasterbench/rasterbench-hard /tmp/ && adb shell /tmp/rasterbench-hard kernels
```
