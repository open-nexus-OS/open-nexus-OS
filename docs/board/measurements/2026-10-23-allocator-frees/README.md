<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# 2026-10-23 — every service's heap frees (TASK-0333 6b-3, Block 4 layer 6)

## Question

Every OS service but three (app-host, vfsd, screencapd) ran its heap as a bump that never frees:
whatever a service allocates per request, per frame or per client stays allocated for its life.
Does a session's work grow the services' memory — and does it stop once every service frees by
size class, a double free refused? And two carries: does an object whose last capability closed
while it was mapped come back at its unmap (6a's gap), and what does a band's first entry into the
GPU's tables cost a frame (6b-2b's carry)?

## Instrument

- new: `KINIT: mm grew (pid= sid= rss= by=)` — at each reap, the kernel names an address space
  that reached a new high of committed bytes by 256 KiB since it was last named (the first sight
  of a process sets its mark silently);
- `execd: reaped pid= … (free= vmos= vmo_bytes=)` — free frames after each app's close;
- `KINIT: mm space pid= sid= rss= dma= reserved=` and `KSELFTEST: mm frames (…)` at the census;
- new: `KSELFTEST: vm unmap reclaim ok (bytes=)`, and `KINIT: vm reclaim (… by=unmap …)`;
- new: `SELFTEST: alloc free refused FAIL (svc= why=twice|foreign size=)` (a service's first
  refused free, red in the lanes' FAIL gate), `apphost: frame arena (… refused= …)`;
- new: `gpud: gpu frame us (… enter=avg/max …)` — the µs a frame spent entering surface pages for
  the GPU, apart from `clean=`.

## Before

- Board cycle 66 (`board--2026-10-23T12-22-07`): free frames at each app's reap fell by 9–20
  (cycle 65: 990 563 → 990 554 → 990 545 → 990 525; cycle 66 alike); no growth line existed.
- QEMU `visible--2026-10-23T13-07-58` (the instrument, the lists still opt-in): the growth lines
  name the boot's warm-up — gpud (+4.2 MB, then +108 MB: the framebuffer at the attach), windowd
  (+0.4 MB), the greeter's app-host (+5.0 MB), `updated` (+2.2 MB), the selftest client
  (+0.4 MB), ingressd (+0.4 MB) — and nothing over the rss loop's ten reaps. QEMU's lanes churn
  too little to tell the lists apart; the board's series is the test.

## Board cycle 67 — decision lines (written before the cycle)

| # | Hypothesis | Decision line |
|---|---|---|
| H1 | every service runs on the lists | the fleet `ready`, no `alloc-fail`; no `alloc free refused` (one is a named bug); each service's census `rss=` within a few pages of cycle 66's (class rounding) |
| H2 | the series no longer grows the services | after the boot, `KINIT: mm grew` names apps (their own spaces) and gpud by the atlas rows it draws — no system service; free frames at the reaps move by fewer than cycle 66's 9–20 a close |
| H3 | an object nobody names dies at its unmap | `KSELFTEST: vm unmap reclaim ok (bytes=8192)` on the board |
| H4 | a band's first entry is measured | `enter=` non-zero in the windows a window opened in, 0 in a steady one |
| H5 | nothing regresses | the cycle-66 series (maximize, tiling, a screenshot, several windows), no new FAIL kind, frame times as cycle 66's |

logd's journal loses its lifetime budget in this step (it refused every record once 256 KiB had
passed through it). That is proven on the host (`journal_keeps_logging_long_past_its_lifetime_total`)
and has no board line: logd names only its first refusal, and the hardening selftest takes that one
on purpose. On the board it shows only as no regression (`SELFTEST: logd hardening rejects ok`,
`SELFTEST: log query ok`).

## Board cycle 67 — results

`board--2026-10-23T16-12-49` (build dev-295a), after a kernel race fix found in QEMU (netstackd
returned to a kernel address: the context switch wrote `sepc` with interrupts on).

| # | Result |
|---|---|
| H1 | ✅ fleet ready, 0 `alloc-fail`, 0 `alloc free refused`; census vs cycle 66: gpud −29 pages, windowd −14, init-lite +17, selftest-client −84, the rest ±1 |
| H2 | ✅ after `desktop revealed` only gpud is named (rss 49.3 → 89.1 MB with three apps open); the closes give it back (free 990 953 → 993 915 → 994 826); no system service; rss-loop reaps −2, −5, then flat (cycle 66: −3, −12) |
| H3 | ✅ `KSELFTEST: vm unmap reclaim ok (bytes=8192)` |
| H4 | ✅ `enter=` non-zero in 6 windows (max 17 813 µs, a window opening), `0/0` in 15 steady ones |
| H5 | ✅ tiling (drag + chord), screenshot, clipboard, modal; FAIL kinds new to cycle 66 (`apphost: ux launch FAIL`, `hidrawd: chain I2 wire send FAIL`) both seen before (10 and 35 of 93 board logs); no netstackd fault |

`board-visible` stays red on the operator's `typed`: typed letters do not show in an app (the
known keyboard defect, carried in the UX lane), so `typed` and `gpu-desktop` are not acked.
