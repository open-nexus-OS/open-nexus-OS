<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# 2026-10-28 — the backing stages: what a page costs to compress on RV64 (TASK-0333 6f-0, RFC-0100 §5)

## Question

RFC-0100 §5 puts two stages between "a purgeable page is dropped" and "memd kills an app": the
anonymous dirty pages of a frozen process compressed into a pool in the kernel, and the compressed
blobs written to the `swap` partition through blkd. Both stand on one number nobody had measured
on this architecture (the RFC's open question R11): what a 4 KiB page costs to compress and to
bring back on the RISC-V core the kernel runs on, and how small it gets. 6f-0 builds the codec
the two stages share — `nexus-compress`, the LZ4 block format, `no_std`, zero dependencies,
allocation-free, with a decoder that checks every length and offset — and measures it where it
will run: in the kernel, on the boot hart, over a sample of the kernel image's pages. The numbers
decide how 6f-1 (the pool) and 6f-2 (the swap stage) are cut.

## Instrument

- kernel: `KSELFTEST: mem compress ok (pages= zero= ratio=% smaller= us_per_page=
  inflate_us_per_page= sample=image)` — up to 64 pages with content out of 1024 stride candidates
  across the kernel image (text, rodata, data, the stack), each copied once, compressed,
  decompressed and compared byte for byte; `zero=` counts the all-zero candidates skipped (a pool
  keeps a zero page as a fact, not a blob — they would flatter the ratio); `ratio=` is blob bytes
  over page bytes of the content pages; `smaller=` the pages whose blob is smaller than a page
  (the ones a pool would keep); the costs are per page, the hart's monotonic clock, integer µs.
- the sample is NOT an app's heap: the kernel image's content pages are mostly machine code,
  the hardest content LZ4 meets (an app's heap — structures, pointers, zeros, text — compresses
  better). The ratio here is a ceiling for the pool's gain, the cost a fair measure of the
  codec's speed on the core.
- host reference (x86-64, release, the same codec over the kernel ELF's 130 content pages, warm,
  20 passes): `ratio=72% us_per_page=18.6 inflate_us_per_page=4.0`.
- as before: `KINIT: milestone 11` (the test runs in the boot's selftest phase — its cost is boot
  time), the FAIL kinds, the ladders (every lane and the board carry the new rung).

## Hypotheses (decided before the board cycle)

| # | Claim | Decision criterion |
|---|---|---|
| H1 | Machine code compresses below a page | `smaller=` equals `pages=`; `ratio=` ≤ 80 % on the image sample |
| H2 | Compressing is cheap enough to squeeze a frozen app whole | board `us_per_page=` ≤ 100 (a 7 MB app-host ≈ 1 800 pages in ≤ 180 ms, off the UI path) |
| H3 | Bringing a page back fits inside a fault | board `inflate_us_per_page=` ≤ 25 (one fault-in is noise against the 16.7 ms frame) |
| H4 | The measurement costs the boot nothing visible | `KINIT: milestone 11` within ±20 ms of cycle 73 (1 853 ms) on the board, of 956 ms on QEMU headless |
| H5 | No regression | FAIL kinds ⊆ cycle 73's, every ladder rung green |

If H2 fails the pool compresses only on memd's order and only hidden-band processes a page at a
time across frames; if H3 fails the fault path decompresses asynchronously (the thread parks,
a kernel worker inflates) — neither changes the codec.

## Results

### QEMU headless (`headless--2026-10-28T09-19-50`, the final kernel; vs cycle 73's `headless--2026-10-27T16-52-23`)

| | before (6e) | 6f-0 |
|---|---|---|
| `KSELFTEST: mem compress ok` | — | `pages=29 zero=227 ratio=47% smaller=29 us_per_page=159 inflate_us_per_page=56 sample=image` |
| `KINIT: milestone 11` | 956 ms | 962 ms |
| FAIL kinds | 4 (QueueFull ×3, declared slot route ×1, two blank forms) | the same 4, equal counts |
| ladder | green | green, 13/13 chain markers |

Reading: 256 stride candidates across the image, 227 of them all-zero (bss, heap, stack — a
pool records a zero page as a fact), 29 with content, every one smaller than a page, the blobs
47 % of the pages. The emulator's cost per page (159 µs compress, 56 µs inflate) is the TCG's,
not the architecture's — the host reference over the same class of pages is 18.6 / 4.0 µs and
the board's number is the one that decides (H2/H3). The first attempt sampled the first 64
content pages of the image at a small stride (`pages=64 zero=0 ratio=74%` — the text alone,
`headless--2026-10-28T08-52-56`), the stride across the whole image is the honest sample.

The host's speed moved this morning: `headless--2026-10-28T08-34-14` and `08-39-22` booted to
milestone 11 at 956/959 ms, the runs from 08:52 at 976–998 ms (load ≈ 2); the compress selftest
is ≈ 6 ms of it (29 pages × 215 µs). H4 is judged on the board.

### QEMU visible (usb-visible) — red for a pre-existing input defect, the 6f-0 rung green in every run

Six runs today, all carrying `KSELFTEST: mem compress ok (pages=29 zero=227 ratio=47% smaller=29
us_per_page=159–160 inflate_us_per_page=56 sample=image)` (one kernel, one hart; the first run
with the small-stride sample read `pages=64 zero=0 ratio=74%`):

| run | tree | red at | reading |
|---|---|---|---|
| `08-56-08` | 6f-0 | `wm chord (zone=close` missing after the thaw | the letter chord lost (below) |
| `09-08-03`, `09-13-45` | 6f-0 | `SELFTEST: ime v2 cjk jp FAIL` | the pointer injector's first event inside the OSK probe — fixed: the injector now waits for `SELFTEST: ime insert ` |
| `09-28-07` | 6f-0 + injector fix | `execd: freeze (pid=` missing | the chord arrived, the hidden app never froze (no grace wake in 150 s) |
| `09-35-54` | **unchanged HEAD** (control) | `cjk jp FAIL`, then `wm chord (zone=minimize` missing | the same two misses without 6f-0 |
| `09-43-47` | 6f-0 + injector fix | `windowd: minimize id=app` missing | the letter chord lost |

The letter chords (Super+Ctrl+M / W) are lost while the arrow chords of the same run arrive —
the board's defect of cycle 73, now on the lane; the chord lands while windowd re-creates the
returned window's surface (`ux input FAIL (to_submit_us=155741)`). Filed for the keyboard package
after 6f (the ledger's "Found by the 6f-0 visible lanes"). Nothing in 6f-0 runs after the boot's
selftest phase, and the control run on HEAD shows the same misses: the lane's red is not 6f-0's.
H5 on QEMU: the FAIL kinds of every run are the 6e set (QueueFull, the declared slot route, the
blank forms) in equal or lower counts.

### Board cycle 74 (`board--2026-10-28T10-02-54`, dev-832104b9, vs cycle 73 `board--2026-10-27T20-36-32`)

| | cycle 73 | cycle 74 (6f-0) |
|---|---|---|
| `KSELFTEST: mem compress ok` | — | `pages=29 zero=227 ratio=47% smaller=29 us_per_page=49 inflate_us_per_page=16 sample=image` |
| `KINIT: milestone 11` | 1 853 ms | 1 730 ms |
| frames free at the census (`KSELFTEST: mm frames`) | 996 672 of 1 043 873 | 996 670 of 1 043 872 |
| watermarks | warn 260 968 / crit 104 386 / emerg 52 193 | the same |
| FAIL kinds | 8 (incl. `imed relay send` ×14, `surface present rejected`, QueueFull) | 5, all in 73's set (`internal`, `gpu gate scene`, declared slot route, the blank forms) |
| `gpud: ux frame` (last windows) | `FAIL (dropped=24 of=32 max_us=123636)` | `FAIL (dropped=32 of=32 max_us=126933)`, `FAIL (dropped=14 of=32 max_us=36091)` |
| `windowd: ux input` to_submit | p50 63.5 ms, p99 382 ms (n=53) | p50 56.8 ms, p99 240 ms (n=45) |
| ladder | 95/95 | 90/96 — rung 91 `inputd: live keyboard route on` missing (below) |
| acks | desktop, pointer, modal, tile, screenshot, gpu-desktop, lifecycle | desktop, pointer, gpu-desktop, lifecycle (the keyboard acts had no act line) |

**Hypotheses.** H1 ✅ every content page smaller than a page, 47 % (the same sample as QEMU —
one kernel image). H2 ✅ 49 µs per page on the board's core: a 7 MB app-host (≈ 1 800 pages)
squeezed whole in ≈ 90 ms, off the UI path. H3 ✅ 16 µs to bring a page back — a fault-in is
noise against a frame. H4 ✅ milestone 11 at 1 730 ms, 123 ms EARLIER than cycle 73 (the
selftest's ≈ 2 ms are inside; the rest is the cycle's own variance — the number to watch stays
the trend). H5 ✅ the FAIL kinds are a subset of 73's with lower counts; no `virtio-blk: timeout`,
no `imed relay send` this cycle (the typing path was never reached — the keyboard, below).

**What the numbers decide (6f-1/6f-2).** The codec's cost is small enough that the pool squeezes
a frozen process WHOLE on memd's order (not a page at a time), and a fault on a squeezed page
inflates SYNCHRONOUSLY in the fault handler (16 µs; no parked thread, no worker). The ratio on
machine code (47 %) is the ceiling for the pool's gain; an app heap compresses better — 6f-1
reports the real ratio per squeezed process (`MM: squeeze (pid= pages= bytes= ratio=)`).

**Found (the keyboard package, after 6f):** the board's keyboard is now dead whole. Both
keyboards enumerate as in 73 (`hidrawd: usb hid device (… role=keyboard)` ×2), hidrawd receives
and normalises the operator's presses (`fps: hidrawd … kbd_batches=4 norm_events=4
wire_batches=4`), yet `inputd: live keyboard route on` never prints — in 73 it did, and the arrow
chords worked. The loss is between hidrawd's wire batch and inputd's keyboard route; the same
boot's pointer path is healthy (`inputd: hid rx hz=264`, taps routed). Together with the lane's
finding (letter chords lost on QEMU while windowd re-creates a surface, the hidden app not frozen
once) this is the first thing the keyboard package measures: the wire batch at inputd's receive.
