<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# 2026-10-22 — the kernel commits on touch (TASK-0333 6b-1, RFC-0100 Phase 1, Block 4 layer 6)

## Question

Every page in this system is committed when it is reserved: a VMO's frames at create, every PTE
at `vm_map`, every RW segment's `.bss` and every stack at exec — and each zeroed. A service's
static heap costs its full size in frames and a memset at every spawn whether or not a byte of it
is used; app-host's 4 MiB heap and 1 MiB arena are reserved in every app process. A contiguous
VMO keeps the whole power-of-two buddy block it was cut from. How much of what is resident is
reserved but never touched, and what does the system hold once a page costs a frame only when it
is first touched?

## Instrument

The kernel's own lines, at the same point of a boot (the board ladder's end, after the operator's
series) and in the lanes:

- `KSELFTEST: mm frames (… free= … vmos= vmo_bytes= dma_bytes= spaces= rss_sum= rss_max=)` —
  frames free, the objects, the sum and the largest of the spaces' RSS;
- `KINIT: mm space pid= sid= rss= dma=` — the twelve largest spaces (`sid` = FNV-1a of the
  service's name: `0x0d05fd724dfcfac3` gpud, `0x600e3345cc7104bb` windowd, `0x7b6096126000c028`
  execd, `0x46f681f38d27984e` init-lite, `0x52c6c4a34ffb3f69` selftest-client,
  `0xd9f31d00b74c1b47` statefsd, `0xbf71cd04b2ec8134` ingressd, `0x43ac3b8ac5ae1d90` updated);
- `apphost: frame arena (… base= peak= …)` — how much of app-host's heap and arena a frame
  touched (`base` = the heap's high-water, `peak` = the arena's);
- new with 6b-1: `KSELFTEST: vm commit on touch ok (reserved= committed= us_per_fault=)` and the
  committed / reserved fields of MmStats v3 (RSS = committed pages).

## The stock tree (board cycle 63, `build/logs/board--2026-10-22T18-51-01`, build `dev-8693`)

`KSELFTEST: mm frames (banks=2 total=1043817 free=975902 … pt_frames=238 vmos=58
vmo_bytes=191418368 dma_bytes=116199424 spaces=41 rss_sum=188178432 rss_max=124026880)`:
67 915 frames in use (265.3 MiB); the spaces' RSS sum 179.5 MiB.

| Space | RSS | of which |
|---|---|---|
| gpud | 124 026 880 | the layout block 107 827 200 (in an order-15 block: 6 443 pages, 25.2 MiB more held and counted nowhere) + the splash 8 294 400 + the cursor |
| app-host (greeter) | 12 029 952 | `.bss` 5 MiB (heap 4 MiB + arena 1 MiB), the shared glyph atlas ~4.25 MB, the copied image |
| app-host (shell) | 12 029 952 | the same |
| execd | 7 278 592 | the embedded app-host ELF (6c) |
| init-lite | 6 365 184 | |
| windowd | 6 017 024 | `.bss` 2 MiB heap |
| selftest-client, two probes | 1 523 712 each | |
| statefsd, ingressd, updated | 1.1–1.3 MB each | a 1 MiB / 768 KiB heap |

What a frame touched of app-host's reserve, the same cycle: the heap's high-water 0.10–0.84 MB of
4 MiB (greeter 0.10, calculator 0.27, stash 0.48, shell 0.65, chat 0.70, settings 0.84), the
arena's peak 0.07–0.52 MB of 1 MiB.

| # | Hypothesis | Decision line |
|---|---|---|
| H1 | app-host's reserve is mostly untouched | an app-host child's committed RSS falls by ≥ 3 MiB from 12.0 MB (its heap's untouched part alone is ≥ 3.1 MiB for every app of cycle 63) |
| H2 | The services' static heaps are mostly untouched | `rss_sum` falls by more than the app-host children's share — the other services' heaps and stacks |
| H3 | A contiguous object's tail is pure waste | `free` rises by ≥ 6 000 frames at the same point of the boot with the tail returned (the layout block's 6 443) |
| H4 | The kernel touches only committed user pages | no `KPGF` in the lanes and on the board; `vm commit on touch ok` |
| H5 | A first touch is cheap | `us_per_fault` (a fault, a zeroed 4 KiB frame, the PTE) ≤ 10 µs on the board |
| H6 | A violation stays fatal | the self-test's `demo.fault` (a load at 0) still ends in `crash report … code=-22`; host `test_reject_*` |

## Results

### QEMU (6b-1 P2, 2026-10-23) — `headless--2026-10-23T09-32-35`, `visible--2026-10-23T09-36-10`

Both lanes green (13/13 chain markers, the FAIL lines of the run before). The same headless lane
before 6b-1 (`headless--2026-10-22T18-05-09`) against after:

| Line | Before | After |
|---|---|---|
| frames free at the late fence | 31 335 | **36 525** (+5 190 = +20.3 MiB) |
| `rss_sum` | 163 770 368 | **142 938 112** (−20.8 MB) |
| app-host (greeter) | 12 140 544 | **7 000 064** committed, 5 029 888 reserved |
| windowd | 6 017 024 | **4 362 240** committed, 1 654 784 reserved |
| execd | 7 389 184 | 6 836 224 |
| updated | 2 785 280 | 2 437 120 |
| selftest-client | 1 515 520 | 1 085 440 |

`SELFTEST: vm commit on touch ok (reserved=262144 committed=65536 us_per_fault=6)` — a mapping of
64 lazy pages reserves 256 KiB and commits nothing, 16 first touches commit exactly 16 pages, 6 µs a
first touch under TCG; `KSELFTEST: userptr unmapped ok` — an unmapped user pointer is `EFAULT`;
`KSELFTEST: mm frames (… filled=)` counts the pages first touches filled; no `KPGF`, no
`vm commit exhausted`. QEMU's gpud layout is page-backed (no contiguous tail to return there).

| # | Verdict on QEMU |
|---|---|
| H1 | confirmed — the greeter's app-host −5.1 MB (≥ 3 MiB) |
| H2 | confirmed — `rss_sum` −20.8 MB, more than the app-hosts' share |
| H3 | the board's (QEMU has no contiguous layout block) |
| H4 | confirmed on QEMU — no `KPGF` in either lane, `vm commit on touch ok` |
| H5 | 6 µs under TCG; the board's number pending |
| H6 | confirmed — `demo.fault` still ends in its crash report; host `test_reject_*` |

### Board (6b-1 P3, cycle 64, `build/logs/board--2026-10-23T09-48-24`, build `dev-747a`)

The operator's series: greeter, calculator, chat (three times, typing), settings, stash — each
opened and closed (`abilitymgr: stop … code=0` every time). At the same late fence as cycle 63:

| Line | Cycle 63 | Cycle 64 |
|---|---|---|
| frames free | 975 902 | **991 583** (+15 681 = +61.3 MiB) |
| `rss_sum` | 188 178 432 | **154 886 144** (−33.3 MB) |
| app-host (greeter) | 12 029 952 | **7 000 064** committed, 5 029 888 reserved |
| windowd | 6 017 024 | **4 358 144** committed, 1 658 880 reserved |
| execd | 7 278 592 | 6 836 224 |
| selftest-client | 1 523 712 | 1 081 344 |
| gpud | 124 026 880 | 123 879 424 (its layout block's tail is in `free`, never was in RSS) |

`SELFTEST: vm commit on touch ok (reserved=262144 committed=65536 us_per_fault=5)`,
`KSELFTEST: userptr unmapped ok`, `KSELFTEST: mm frames (… committed=186028032 filled=891)`; no
`KPGF`, no app crash (the self-test's `demo.fault` and `demo.minidump` only), no `alloc-fail`, no
`vm commit exhausted`, no runtime `ERR`. Observed: `settingsd: event send FAIL chan=11
err=QueueFull` six times in the boot's registration burst (the QEMU lanes have shown it every run,
6a's note; the subscriber resyncs — `inputd: settings watch resync`), not on cycle 63's board.

| # | Verdict |
|---|---|
| H1 | confirmed — an app-host child −5.0 MB (≥ 3 MiB) |
| H2 | confirmed — `rss_sum` −33.3 MB, more than the app-hosts' share |
| H3 | confirmed — +15 681 frames free (≥ 6 000; the layout block's tail is 6 443 of them) |
| H4 | confirmed — no `KPGF` on four harts, `vm commit on touch ok` |
| H5 | confirmed — 5 µs a first touch (≤ 10) |
| H6 | confirmed — `demo.fault` ends in its crash report (`code=-22`) |

## Verdict (6b-1)

Memory is committed where it is touched: every static heap, the frame arena and every stack cost
the pages used, a contiguous object keeps exactly its frames, and a syscall's bad pointer is
`EFAULT`. 6b-1 is done; 6b-2 (the layout block split, the arena reserved for the largest frame)
and 6b-3 (the allocator frees by default) follow.
