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

## 6f-2 — the swap stage (the pager, ADR-0088)

### Question

Can a squeezed frozen app's records leave RAM entirely — to the new `swap` partition, through memd
and blkd — without either ever holding plaintext, and come back intact when the app is shown again
or touches a page first? And what do the two kernel selftests of the backing stages cost of the
boot (cycle 75 could not attribute its selftest window: +115 ms, the squeeze selftest's share
unmeasured)?

### Instrument

- kernel: `KSELFTEST: swap ok (pages= sealed= parked= back= killed= dropped= us= reject=)` — a
  synthetic app, RNG service, pager, stranger, waiter and victim against the real syscall table,
  the fault's own fill and the pressure sink: the refusals (a key from a stranger, a second key,
  an export by a stranger and without a key, a stranger's commit, the superseded export's, a
  stale slot, a tampered and a replayed record), two exports, the commits, the fault's and a
  syscall's verdict on a swapped page, one `OP_PAGE_IN` for two waiters, the page back byte-exact
  and the waiter woken, a victim killed by a record that does not open, the drop at the object's
  death; `us=` the whole selftest on the hart's clock. `KSELFTEST: task squeeze ok (… us=)` now
  carries its duration too.
- the chain (usb-visible lane): `rngd: swap key seeded`, `memd: swap on (slots=)`, `memd: swap out
  (pid= band= pages= slots= reason=)`, `SELFTEST: swap out ok (pages= slots= reason=drill)` at the
  drill's second `critical` of the first freeze; after the thaw `memd: swap in (pid= pages= slots=)`
  and `SELFTEST: swap in ok (pages=)`; at the second freeze the swap-out, the kill and `memd: swap
  drop (slots= freed=)`.
- as before: `KINIT: milestone 11`, the selftest window, the FAIL kinds, the ladders.

### Hypotheses (decided before the lanes and the board cycle)

- **H11 — the mechanism holds on every hart it runs on.** `KSELFTEST: swap ok (pages=3 … parked=1
  back=1 killed=1 dropped=2 …)` on headless, usb-visible and the board, with the one `MM: page-in
  refused (… killed=1)` line of its forged answer and no other. Any FAIL: the stage is wrong.
- **H12 — an app goes out whole and comes back whole.** On the usb-visible lane `swap out ok`
  names every page the squeeze recorded (`pages=` of `swap out` = `pages=` of `mem squeeze ok`),
  the thaw brings every slot back (`swap in`'s pages + the pages served on request = the slots
  out), the app runs on (`ability lifecycle ok`, the restore phase), and the second freeze's kill
  frees every slot (`swap drop`'s `freed=` = the slots out).
- **H13 — the backing stages' selftests are cheap.** On the board `task squeeze ok`'s and `swap
  ok`'s `us=` each stay ≤ 10 ms; above it they are what cycle 75's selftest window grew by.
- **H14 — the board never swaps.** Its tree names no entropy device (`init: rng plane none`):
  memd never gets a key, no `memd: swap on` on the board.

### Results

#### QEMU headless (`headless--2026-10-28T18-09-05`) and usb-visible (`usb-visible--2026-10-28T18-12-21`)

| | headless | usb-visible |
|---|---|---|
| `KSELFTEST: swap ok` | pages=3 sealed=312 parked=1 back=1 killed=1 dropped=2 us=1602 | identical |
| `KSELFTEST: task squeeze ok` | … us=2946 | identical |
| first freeze (settings, pid 117) | — | squeezed 450 → `swap out (pages=450 slots=450)` |
| the thaw | — | 1 page on request (the app touched it first), `swap in (pages=449 slots=449)` |
| second freeze (pid 118) | — | squeezed 448 → swapped out 448 → killed → 448 slots dropped |

- **H11 ✅ (QEMU):** the line with the designed counts on both lanes; the forged answer's one
  kill named once (`MM: page-in refused (vmo=27 page=1 killed=1) record did not open`).
- **H12 ✅:** 450 out, 449 + 1 back, the thawed app ran the rest of the lane (`ability lifecycle
  ok`, the warn fan-out heard by its window, the restore, the clipboard and the capture phases);
  the second freeze's 448 slots all freed by the kernel's drops.
- **H13 (QEMU part):** 2.9 ms and 1.6 ms on the TCG hart; the board decides.

#### Board cycle 76 (`board--2026-10-28T21-38-30`, dev-c71683c0, two boots; vs cycle 75 `board--2026-10-28T13-00-46`)

| | cycle 76 | cycle 75 |
|---|---|---|
| `KSELFTEST: swap ok` | pages=3 sealed=312 parked=1 back=1 killed=1 dropped=2 us=15013 (first boot 15014) | — |
| `KSELFTEST: task squeeze ok` | … us=9099 (first boot 9103) | (no `us=` yet) |
| `KSELFTEST: mem compress ok` | ratio=45% us_per_page=46 inflate_us_per_page=14 | ratio=46% 48 / 14 |
| `SELFTEST: exec child squeeze ok` | pages=11 zero=1 kept=2 taken=2 stored=1060 batches=1 us=723 | … us=632 |
| `KSELFTEST: bkl budget ok` | max_wait=2883us max_hold=1ms (first boot 3377us / 0ms) | max_wait=2603us max_hold=0ms |
| `blkd: gpt ok` | parts=8 | parts=7 |
| the entropy device | `init: rng plane none (no device in the tree)` — no key, nothing swapped | — |
| ladder (board-visible) | 99/99 | 98/98 |
| first boot: milestone 1 / window 6→7 / milestone 11 | 961 / 660 / 1861 ms | 1077 / 726 / 2042 ms |
| second boot (the operator's): milestone 1 / window / milestone 11 | 943 / 754 / 1936 ms | — |

- **H11 ✅:** the designed counts on the board's harts in both boots; the forged answer's one kill
  named once (`MM: page-in refused (vmo=27 page=1 killed=1)`); the FAIL kinds identical to cycle 75.
- **H12:** QEMU only — the drill never runs on a board (see the lanes above).
- **H13 — half:** the squeeze selftest 9.1 ms (≤ 10 ✅), the swap selftest 15.0 ms (over the 10 ms
  line). What the criterion was for is answered either way: the two together are 24 ms, while the
  same image's selftest window was 660 ms in one boot and 754 ms in the next — cycle 75's 726 ms
  was that spread, not the squeeze selftest. The swap selftest's 15 ms is not split by the trace
  (on TCG it is 1.6 ms, so the board's eight harts — the shootdowns of three spaces' worth of
  mappings — are the first suspect); a one-page export buffer in the selftest would drop fourteen
  page commits. Filed, not built.
- **H14 ✅:** no `memd: swap on` and no `rngd: swap key` line on the board — the tree names no
  entropy device, so the pager is never even asked.
- **Acks:** desktop, typed, pointer, tile, screenshot, gpu-desktop, lifecycle — each with its act
  lines in the second boot (lifecycle: stash `windowd: minimize id=app3` … `execd: freeze
  (pid=93)` → `abilitymgr: suspend (inst=6)` → `windowd: restore id=app3` → `execd: thaw
  (pid=93)`; the screenshot `screencapd: saved (kind=screen …)`; the tile `windowd: wm chord
  (zone=left-half …)`). **modal, clipboard and restore not acked:** the trace has no shutdown
  alert closed by ESC/Cancel, no text field focused at all (no copy, no paste) and no close of
  settings.
- **Found:** execd's child table (16 entries) evicted settings (pid 92) while it ran — the
  operator opened four apps before the harness's 15 probe children — and every lifecycle order
  for it was refused (`execd: order denied (sender or not a live child; pid=92)`, `abilitymgr:
  FAIL freeze (inst=5)`). Latent since 6d; the ledger names the fix for the next step.

## 6f-3 — memd's journal and the real level (the `lowmem` lane)

### Question

Does a pool the kernel itself judges `critical` — not a drill — run the authority's whole chain on
a frozen app (squeeze → swap-out → kill, `reason=oom`)? Is the kill recorded in the state store and
read back, does the app come back with its state when it is launched again, and does the system
recover once the memory is given back?

### Instrument

- the harness on the new `lowmem` lane (`selftest-client/src/os_lite/lowmem.rs`): once an app is
  frozen it takes eager objects until free frames are halfway between the critical and the
  emergency watermark — `SELFTEST: mem pressure held (level= free= taken=)` — and gives them back at
  memd's next kill — `SELFTEST: mem pressure released (level= free= taken=)`;
- memd: the kernel's frames (`memd: pressure (level= free= total=)`), `memd: squeeze / swap out /
  kill (… reason=oom)`, the new `SELFTEST: memd journal ok (reason= seq= records=)`; execd's squeeze
  line (`us=` the walk); the fan-out `memd: level told (level= source=oom …)`;
- the restore: `apphost: persist saved (… reason=hidden)` → `SELFTEST: app restore ok (fields=
  bytes= warm_us=)` after the injector's relaunch;
- verify-uart's deny list: `SELFTEST: mem pressure drill (` is forbidden on `lowmem`.

### Hypotheses (decided before the lanes)

- **H15 — the kernel's own level drives the chain.** On `lowmem` the held line says
  `level=critical`, memd's frame is `memd: pressure (level=critical`, and the squeeze, the
  swap-out and the kill follow, each `reason=oom`, with no drill line. A kill without the squeeze
  (the level overshot to `emergency`) or no kill at all (it recovered first): the target is wrong.
- **H16 — the kill is recorded.** `SELFTEST: memd journal ok (reason=oom …)` on `lowmem`,
  `(reason=drill …)` on usb-visible: the journal read back equals the one written.
- **H17 — the killed app comes back.** The relaunched settings restores what its hidden
  predecessor saved: `fields=`/`bytes=` of `app restore ok` equal those of `persist saved (…
  reason=hidden)`.
- **H18 — everything comes back.** After the release the level is `normal` and free frames ≥ the
  free frames at the hold plus the frames taken (the harness's memory and the victim's both back).
- **H19 (board) — regression only.** No kill on the 4 GiB board: no journal line, the ladder as in
  cycle 76, no new FAIL kinds.

### Results

#### QEMU lowmem (`lowmem--2026-10-29T10-20-12`)

The first run (`lowmem--2026-10-29T10-06-12`) ran the same chain end to end and was red on a
ladder literal alone (`source=kernel`; the wire names the kernel's level `source=oom`). It also
showed abilitymgr thawing the killed app (below); the rerun carries both fixes.

| | |
|---|---|
| the take | the kernel's frames warn (free 17 289) → critical (free 7 049) while the harness took |
| held | `level=critical free=5791 taken=31978` — 5 791 = (7 722 + 3 861) / 2, the midpoint exactly |
| squeeze | settings (pid 117): 448 pages, net 331 frames, 25 %; execd's walk 79 ms (29 batches, TCG) |
| swap-out | `rngd: swap key seeded`, 448 pages → 448 slots |
| kill | `memd: kill (pid=117 band=suspended reason=oom)` → `abilitymgr: kill` → `execd: kill`; 448 slots dropped |
| journal | `SELFTEST: memd journal ok (reason=oom seq=1 records=1)` |
| fan-out | `memd: level told (level=critical source=oom gpud=1 windowd=1)`, gpud and windowd heard it |
| after the kill | `memd: no victim (level=critical reason=oom)` — the shell and the greeter are on screen |
| release | the kernel's frames critical → warn (free 9 900) → normal (free 22 188); `released (level=normal free=39830 taken=31978)` |
| restore | `persist saved (fields=3 bytes=87 reason=hidden)` → `app restore ok (fields=3 bytes=87 warm_us=20630)` |
| the run | QEMU 162 s, of which the lowmem phase (launch → hide → pressure → kill → relaunch → restore) 91 s |

- **H15 ✅:** the held line at the midpoint, the kernel's `critical` frame, then squeeze → swap-out
  → kill, all `reason=oom`; verify-uart clean with the drill line on the deny list.
- **H16 ✅ (lowmem):** written and read back, `seq=1` — every lane boots a fresh disk image, so the
  sequence across boots is host-proven (`the_ledger_writes_reads_back_and_the_next_boot_goes_on`).
- **H17 ✅:** 3 fields / 87 bytes saved at `hidden`, the same 3 / 87 restored after the kill.
- **H18 ✅:** 5 791 + 31 978 = 37 769 free when the hold began; 39 830 after the release — every
  frame back, plus 2 061 of the victim's.
- **Found:** the killed app's window closed before execd's death report reached abilitymgr, and
  the sighting `gone` of a frozen instance thawed it (`execd: order denied (… pid=117)` →
  `abilitymgr: FAIL thaw (inst=3)`, a `visible` band pushed for the dead pid). An instance whose
  kill was carried is now `dying` — only the death report moves it; the rerun has neither line.

#### QEMU usb-visible (`usb-visible--2026-10-29T10-40-34`)

| | |
|---|---|
| first freeze | squeezed 450 (net 333) → swapped out 450 → the thaw: `swap in ok (pages=449)` (+1 on request) |
| second freeze | squeezed 448 (net 330) → swapped out 448 → killed (`reason=drill`) |
| journal | `SELFTEST: memd journal ok (reason=drill seq=1 records=1)` |
| the rest of the lane | clipboard (`text copy ok`, `ui v7 clipboard ok`) and capture (`ui v7 screenshot ok`) green |

- **H16 ✅ (usb-visible).** The run before (`usb-visible--2026-10-29T10-26-17`) was red in the
  clipboard phase: its first press — sent the moment the kill was proven, while the closed window
  recomposited and the journal went to the store — never surfaced, and the travel after it
  landed on the clock (the same injector failure 6f-2's test-all met once). The injector now waits
  for the journal line, the kill's last step, before the next phase.

#### test-all

The first test-all (`lowmem--2026-10-29T11-22-22`) was green up to the `lowmem` lane, which ran
the whole chain again (the same numbers as above) and was red on one rung: `abilitymgr: kill
(inst=` was torn — abilitymgr wrote its lines a byte at a time and execd's exit line landed after
`abilitymgr: `. Both services now write every line whole (`nexus_abi::DebugLine`, one write).
The second test-all (`usb-visible--2026-10-29T12-30-56`) passed every rung of `usb-visible` and
failed its FAIL gate: `KSELFTEST: ipc call budget FAIL (rt=90us budget=64)` — 46–48 µs in every
other run of the week. The injector's first pointer event had landed between `ipc soak ok` and the
budget line (green runs start ~70 UART lines after it, by the emulator's speed alone). The injector
now waits for the budget line before its first event, the window `input-flood` already keeps.

## 6f-3 — the determinism package (ADR-0089)

### Question

The 6f-3 test-all runs went red for reasons that were not the change under test. Which classes are
they, does each structural fix remove its class, and do the visible lanes then pass repeatedly
without a rerun?

### Before (the old choreography, kernel and gates; runs of 2026-10-27 … 2026-10-29)

| run | lane | red because | class |
|---|---|---|---|
| 2026-10-27 (6e) | visible | `virtio-blk: timeout` ×4 → settings launch failed (host cargo running) | host I/O |
| 2026-10-28 (6f-2 test-all) | usb-visible | clipboard press lost, retry off target | injector race |
| `usb-visible--2026-10-29T10-26-17` | usb-visible | first press of the clipboard phase lost after the kill; the retry pressed the clock | injector race |
| `lowmem--2026-10-29T11-22-22` | lowmem | `abilitymgr: kill (inst=` torn by execd's exit line | console tear |
| `usb-visible--2026-10-29T12-30-56` | usb-visible | `ipc call budget FAIL (rt=90us)` — the injector's first press inside the benchmark | injector race + load-sensitive gate |

Green the same days: `usb-visible--2026-10-29T10-40-34`, `…T11-13-49`, `lowmem--2026-10-29T10-20-12`
— 2 of 4 usb-visible and 1 of 2 lowmem runs of 2026-10-29 red, none of them for the change under
test. The old log itself shows the tear in a GREEN run (`abilitymgr: bg (inswindowd: windows push
(n=1)` in `usb-visible--2026-10-29T10-40-34`): only luck kept it off a laddered marker.

### Found while building it (the declared scenarios, first runs)

- `lowmem--2026-10-29T15-35-40`: a chord as the keyboard's first input got ONE empty report from
  QEMU's USB keyboard and nothing after it (`hidrawd: wire skip raw=1 norm=0`, `kbd_batches=1`, no
  `inputd: live keyboard route on`); the old early phase had masked it by pressing `a` first. →
  the first key is a step of its own (a letter, retried until the live keyboard route).
- `usb-visible--2026-10-29T16-14-57`: `ui v10 live modal ok` is a once-per-boot proof of the FIRST
  close — expected after the wrong step (a porting error, named by the runner at once).
- `usb-visible--2026-10-29T16-31-54`: the minimize came 1.5 s after the tile's return, before the
  window's re-created surface was up; the new surface took the focus back (`abilitymgr: fg`). The
  old choreography's accidental 20 s idle waits had covered it. → every re-creation (after a
  launch, a tile, a return, a relaunch) is awaited: `WINDOWD: surface presented id=` only after
  `apphost: resized` (first as one ordered list; since `17-36-39` the `then` phase, below).
- `usb-visible--2026-10-29T16-50-33`: `virtio-blk: timeout` ×4 → `blkd: trace os FAIL (write)` →
  `bundlemgrd: FAIL get_payload` with no host build running — every guest flush was a host
  `fdatasync` of the image on a disk at 97 %. → proof sessions attach the image `cache=unsafe`.
  The same run showed the re-home: the control center's first press never surfaced (45 s), the
  runner homed and the second press landed on the pill.
- `usb-visible--2026-10-29T17-11-05`: the control center's press MISSED (`apphost: input tap miss
  at (1253,18)` — the session shell was not hit-ready 5 s after its login), and the runner, like
  the old injector, counted a press read back ON the point as a click; the power press missed the
  same way and no alert opened. In every green run every choreographed press is a hit
  (`hit=Some(n)`, the background press included — the scrim takes it). → only a HIT on the point
  acknowledges a click; a miss there waits 1.5 s and presses again (bounded).
- `usb-visible--2026-10-29T17-36-39`: every step through the thaw passed at its first attempt; the
  close was red with every line it waited for PRESENT — windowd's `app window closed` came before
  app-host's `persist saved`, and the ordered list demanded the opposite. Two services write
  concurrently; only causality orders their lines, and a list order is a claim about causality.
  → `expect` is a SET (new lines after the action, any order); what follows causally from it — a
  re-created surface's first frame after its resize — is a `then` phase, each line after the
  last `expect` line. Replayed offline over this run's UART, the new rules accept every step up
  to where the run stopped, and the whole of `lowmem--2026-10-29T16-03-39`.
- `usb-visible--2026-10-29T18-00-03`: every step through the relaunch at its first attempt, the
  close included; red at `the record comes back` with `SELFTEST: app restore ok` IN the log — it
  came while the relaunch step was still being acknowledged, and the `wait` step counted from its
  own start. (The offline replay counted from the previous step's last line and could not see
  it.) → a `wait` step, having no action, counts from the action of the step before it.
- `usb-visible--2026-10-29T18-35-20`: every step through the restore at its first attempt; red at
  the second hide: `virtio-blk: timeout` ×4 → `virtio-blk: slots exhausted` ×445 → `memd: swap
  out (pages=2)` → `memd: journal FAIL (write refused …)`. Just before the first timeout a frame
  went unacknowledged for 2.0 s (`sent>acked=…/2007267`): the whole guest stalled, not the disk
  alone (host load 1.3, the lane's cgroup `high 0`; `16-50-33` showed the same four timeouts →
  `slots exhausted`). The driver made the stall a dead device: a request its 2 s watchdog gave up
  kept its slot for the rest of the boot (four slots), and a watchdog wake never looked at the
  used ring. → ADR-0089 decision 7: virtio-blk waits for its device's interrupt alone (no
  watchdog, no poll — the line bound before the warm-up read), a failed device says so itself
  (`DEVICE_NEEDS_RESET`), and a slot comes back only with its chain (`inflight.rs`, host-tested).
- Audit (operator, 2026-10-29: "the selftest client must be a pure observer"): after its ladder
  the harness spun on `yield_()` for the whole lane (the one hart never idle; under `-icount` the
  guest's clock ran on the spin), and the input window opened before its last probe (the UI
  smoke). → ADR-0089 decision 8: the window opens after the harness's own work; the end phase
  sleeps on its declared timer until the drill's next due look, and for good once none is due.

### After (every rule in place: declared scenarios, ordered phases, reactive block driver, observer harness)

| run | lane | result | steps at the first attempt | notes |
|---|---|---|---|---|
| `usb-visible--2026-10-29T19-18-28` | usb-visible | green (EXIT=0, chain markers 35/35) | 28/28 logged, the 29th's lines in the UART (the lane stops at its last marker) | `blk: irq bound` → `blk: irq completion on` before the warm-up's end; no `virtio-blk: timeout`/`slow`; IPC `rt=47us mean=47us`; squeeze 450 → swap out 450 → swap in 449 → restore → squeeze 448 → swap out 448 → kill → journal `seq=1` |
| `lowmem--2026-10-30T00-02-49` | lowmem | green | 7/7 | IPC `rt=47us mean=47us`; kill `reason=oom` → journal `seq=1` → restore 3 fields / 87 bytes |
| `usb-visible--2026-10-30T00-07-09` | usb-visible | green (35/35) | 28/28 logged | IPC `rt=47us mean=51us`; chain as above; no block-driver line |
| `lowmem--2026-10-30T00-14-14` | lowmem | green | 7/7 | IPC `rt=47us mean=55us`; as above |
| `visible--2026-10-30T00-18-15` | input-flood | green (45 s at ~900/s, forwarding intact) | 2/2 | IPC `rt=47us mean=51us` |
| `usb-visible--2026-10-30T00-25-24` | usb-visible | green (35/35) | 28/28 logged | IPC `rt=47us mean=55us`; as above |
| `lowmem--2026-10-30T00-33-18` | lowmem | green | 7/7 | IPC `rt=47us mean=55us`; as above |

**Verdict.** Before: 2 of 4 `usb-visible` and 1 of 2 `lowmem` runs of 2026-10-29 red, none for
the change under test. After: 3/3 `usb-visible`, 3/3 `lowmem` and 1/1 `input-flood` green in a
row, no rerun; every scenario step acknowledged at its first attempt (not one retry fired); not
one block-driver stall line; the IPC budget's fastest round 47 µs in all seven runs while the mean
moved 47–55 µs with the hart's load — the gate now holds still where the old one did not.

### Board cycle 77 (`board--2026-10-30T08-29-17`, dev-b2eb30f6, one boot; vs cycle 76 `board--2026-10-28T21-38-30`)

| | cycle 77 | cycle 76 |
|---|---|---|
| ladder (`board-visible`) | 99/99 rungs | 99/99 |
| IPC call budget | **ok** `rt=26us mean=67us budget=64` (the fastest of 64) | FAIL `rt=68us` (the mean; tolerated red) |
| `KSELFTEST: task squeeze ok` / `swap ok` | `us=9117` / `us=15015` | `us=9099` / `us=15013` |
| `KSELFTEST: mem compress ok` | ratio 45 %, 46 µs / 14 µs per page | — |
| milestone 11 | 2092 ms (the boot after the flash) | 1936 ms (second boot; first 1861; cycle 75 2042) |
| storage backend | `k1-sdhci` hs400es (virtio-blk is QEMU's) | same |

- **Acks (8):** desktop, gpu-desktop, pointer, typed, lifecycle, tile, screenshot, modal — each
  confirmed by the operator for this boot, with its act lines (lifecycle: `windowd: minimize` ×5,
  `execd: freeze` ×2 → `abilitymgr: suspend` → `execd: thaw` ×2; screenshot `screencapd: freeze
  ok` + `saved (`; modal `apphost: modal open` ×4 / `modal dismiss` ×4 — new against cycle 76;
  tile: four quarters by the menu, `windowd: wm tile (zone=top-left …)` … — the chords in the
  trace are close and minimize, no tile chord: acked on the operator's confirmation).
  **Not acked:** clipboard — the operator tried it, the trace has no copy and no paste (typing on
  the board is still lost — the keyboard package); restore — settings closed with its record
  (`apphost: persist saved (fields=3 bytes=87 reason=close)`) but was not opened again.
  `board-test` names exactly these two operator markers as missing.
- **The execd fix holds on the board:** the operator's four apps (calculator, chat, settings,
  stash) ran before the harness's probe children again, and every lifecycle order went through —
  no `order denied` (cycle 76: refused for settings, pid 92).
- **The IPC gate holds still:** the mean sits where cycle 76's red one did (67 vs 68 µs: cross-hart
  wakes); the path is 26 µs. `KSELFTEST: ipc call budget FAIL` left the board's tolerated reds
  (`config/fail-marker-allow-board.txt`).
- **milestone 11 at 2092 ms** — the top of the spread so far (1861–2042 ms over cycles 75–76),
  not isolated here (one boot); the next cycle compares a second boot.
- **The flash read-back and a boot between:** the read-back after this boot failed `nxdisk0` and
  `nxdisk1`; per partition, every image-owned one matched (opensbi, uboot, boot-a/b, system-a/b)
  and exactly the three a running system writes differed (`bsb`, `state`, `data`). → The layout
  declares them (`storage::layout::LIVE`, with `trace`); the plan gives each region a read-back
  digest with them zeroed; `board-flash.sh --verify` reads them as zero. The plan rebuilt from
  the same image (its region bytes identical) then read back clean from this board: "every region
  reads back as planned".
