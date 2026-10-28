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

**Corrected 2026-10-28 (6f-1):** the reading of the `08-56-08`, `09-28-07`, `09-35-54` and
`09-43-47` rows was wrong. All six runs were cut by the 240 s budget a plain `just test-os
usb-visible` inherits (`qemu.stderr`: `terminating on signal 15 … (timeout)`) — the "lost" chord
was never processed before QEMU was stopped, the "missing" freeze was inside the grace when it
was. The lane runs as `just ci-os-usb-visible` (900 s). The `cjk jp` rows stand (the guest's own
FAIL lines; the injector fix holds).

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

---

## 6f-1 — the squeeze (the pool, ADR-0087)

### Question

Does the pool hold what 6f-0's numbers promised, on a real process, without a stale translation
and without holding the kernel lock for a whole process? What a squeeze can reach was read from
cycle 74's census before anything was built: an app-host holds 7.0–8.8 MB resident, 6.68 MB of
it the shared image (clean, never compressed); its private 0.3–2.2 MB is the lazy `.bss` (the
heap's bump arena, 8 MiB reserved) and its stack — exactly what the squeeze takes.

### Instrument

- kernel: `KSELFTEST: task squeeze ok (pages= zero= kept= store= stored= inflated= reject=)` —
  a synthetic parent and child against the real syscall table: 4 pages of text, 1 of zeroes, 1
  of noise in the child's own object, one object another space maps, one a device was told of;
  the refusals (a stranger's order, a running child's), the batches until `0`, the PTEs, every
  page back through `populate::fill` (the user fault's path) byte-exact, the stores empty again,
  a second squeeze's records gone with the object.
- service launcher: `SELFTEST: exec child squeeze ok (pages= zero= kept= taken= stored= batches=
  us=)` — the recv-wake probe child writes 8 pages of text, 2 of zeroes and 2 of noise into its
  heap before its third park; execd freezes it, squeezes it (`us=` the parent's clock over all
  batches), thaws it; the child checks every byte and answers `woke` only when the pattern came
  back whole.
- the drill (usb-visible lane): `execd: squeeze (pid= pages= zero= kept= taken= stored= batches=
  us=)`, `abilitymgr: squeeze (inst= pages=)`, `memd: squeeze (pid= band= …)`, `SELFTEST: mem
  squeeze ok (pages= net= ratio=% reason=drill)` — a real app-host frozen by the lifecycle phase,
  squeezed before the injector thaws it.

### Hypotheses (decided before the lanes and the board cycle)

- **H6 — the mechanism holds on every hart it runs on.** `KSELFTEST: task squeeze ok (pages=4
  zero=1 kept=1 … reject=stranger,running,shared,device)` on headless, usb-visible and the board.
  Any FAIL: the pool is wrong.
- **H7 — a real process gets every byte back.** `SELFTEST: exec child squeeze ok` with pages ≥ 6,
  zero ≥ 1, kept ≥ 1 on all three; no `RECVWAKE: FAIL the squeezed pattern came back wrong`.
- **H8 — the batch bound is right.** The probe's squeeze cost per examined page on the board
  (`us` over pages + zero + kept) stays ≤ 100 µs (2× 6f-0's 49 µs compress); above it the
  16-page batch is not ≈ 0.8 ms and is re-cut. No `bkl budget FAIL` on the board.
- **H9 — an app's heap compresses better than machine code.** The drill's `ratio=` on the
  usb-visible lane is below 6f-0's 47 % ceiling and `net=` > 0; ≥ 47 %: the pool's value for
  apps is re-judged before 6f-2. The thawed app works: `SELFTEST: ability lifecycle ok` and the
  warn fan-out reaching it (`apphost: mem pressure (level=warn`).
- **H10 — what the wait costs.** `execd: squeeze (… us=)` of the app on QEMU (TCG) names how long
  memd and abilitymgr wait; the board's only squeeze is the probe child's (the drill never runs
  on a board).

### Results

#### QEMU headless (`headless--2026-10-28T12-07-42`) and usb-visible (`usb-visible--2026-10-28T12-38-24`)

| | headless | usb-visible |
|---|---|---|
| `KSELFTEST: task squeeze ok` | pages=4 zero=1 kept=1 store=1 stored=386 inflated=4 | identical |
| `SELFTEST: exec child squeeze ok` | pages=11 zero=1 kept=2 taken=2 stored=1060 batches=1 us=1285 | … us=1283 |
| `KINIT: milestone 11` | 967 ms (6f-0: 962) | 990 ms |
| the drill's first squeeze (settings, pid 110) | — | pages=450 zero=0 kept=0 taken=117 stored=463155 batches=29 us=76868 |
| `SELFTEST: mem squeeze ok` | — | pages=450 net=333 ratio=25% (and pages=448 net=330 ratio=25% at the second freeze) |

- **H6 ✅ (QEMU):** the line with the designed counts on both lanes; no `MM: FAIL inflate`.
- **H7 ✅ (QEMU):** the child answered `woke` after the thaw (its pattern whole) on both lanes.
- **H8 (QEMU part):** 14 examined pages in one batch, 1.28 ms on TCG; the board decides.
- **H9 ✅:** an app-host's heap and stack: 25 % (`ratio=`), well under 6f-0's 47 % ceiling on
  machine code; 1.8 MB became 468 KB of records, 333 frames (1.3 MB) went back net. The thawed
  window ran on: `SELFTEST: ability lifecycle ok`, the queued `critical` and the next `warn` heard
  (`apphost: mem pressure (level=…`), the restore phase, the second freeze's squeeze, the kill.
- **H10 (QEMU):** one app's squeeze is 77 ms on the TCG hart in 29 batches (≈ 2.7 ms a batch,
  ≈ 171 µs a page) — what memd and abilitymgr waited.
- **The lane's verdict needs its recipe:** `just ci-os-usb-visible` (900 s). The two earlier runs of
  the day were a 240 s plain `test-os` run and a run whose emulator a leftover monitor of the
  previous run killed (see the ledger's "Found by the 6f-1 lanes"); neither says anything about
  the guest.

#### Board cycle 75 (`board--2026-10-28T13-00-46`, dev-6c04a43c, one cold boot; vs cycle 74 `board--2026-10-28T10-02-54`)

| | cycle 75 | cycle 74 |
|---|---|---|
| `KSELFTEST: task squeeze ok` | pages=4 zero=1 kept=1 store=1 stored=386 inflated=4 reject=stranger,running,shared,device | — |
| `SELFTEST: exec child squeeze ok` | pages=11 zero=1 kept=2 taken=2 stored=1060 batches=1 us=632 | — |
| `KSELFTEST: mem compress ok` | ratio=46% us_per_page=48 inflate_us_per_page=14 | ratio=47% 49 / 16 |
| `KSELFTEST: bkl budget ok` | max_wait=2603us max_hold=0ms | (ok) |
| ladder (board-visible) | 98/98 | 90/96 (the keyboard route missing) |
| first boot: milestone 1 / window 6→7 / milestone 11 | 1077 / 726 / 2042 ms | 963 / 611 / 1814 ms |

- **H6 ✅:** the designed counts on the board's harts, the same as on both lanes.
- **H7 ✅:** the probe child answered `woke` after the thaw — its pattern came back whole on the
  board's core.
- **H8 ✅:** 632 µs for the batch that examined 14 pages — 45 µs a page including the syscall, the
  stores' frames and the shootdown, under the 100 µs criterion; `bkl budget ok` (no FAIL).
- **H9, H10:** QEMU only — the drill never runs on a board, and the board's pool stays far above
  `critical`.
- **Boot time, as measured and not yet attributed:** milestone 11 at 2042 ms against cycle 74's
  first boot at 1814 ms. Half of it lies before the kernel's first milestone (the loader: 963 →
  1077 ms — no 6f-1 code runs there); the selftest window grew 611 → 726 ms, 35 ms above the
  highest of the seven board runs before it (499–691 ms, 2026-10-25 … 2026-10-28). The new selftest's own share is
  not isolated by the trace (no per-line times); on QEMU it costs 5 ms. 6f-2 puts its duration into
  its own marker so the next cycle measures it.
- **The keyboard was alive this cycle** (`inputd: live keyboard route on`, 25 reports with key
  batches, the Super+Ctrl chords for close, halves, minimize and restore all arrived) — after cycle
  74's dead keyboard; the typing path's old `inputd: imed send FAIL` / `WINDOWD: FAIL imed relay
  send` lines are back with it (red since cycle 64, the keyboard package's).
- **Acks:** desktop, typed, pointer, modal, tile, clipboard, screenshot, gpu-desktop, lifecycle —
  each with its act lines in this boot's trace (lifecycle: `windowd: minimize id=app2` → `execd:
  freeze (pid=112)` → `abilitymgr: suspend (inst=5)` → `windowd: restore id=app2` → `execd: thaw
  (pid=112)`). **restore not acked:** settings was frozen and thawed but not closed and reopened
  this boot (no `apphost: persist saved (… reason=close)`, no `SELFTEST: app restore ok`) — the act
  stays open for the next cycle.
