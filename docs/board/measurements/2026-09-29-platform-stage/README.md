# 2026-09-29 — the platform stage is never signalled on the board (TASK-0327B P4 H0d)

## Question

`scripts/board-test.sh` (H0c) judged the day's board trace one rung short: `stage: platform`
never appears, while every platform-floor service printed its `ready` line. Where do the
`@ready` frames go?

## Instrument

- Trace: `build/logs/board--2026-09-29T*/uart.log` (H0b image dev-2df7b50b, pulled with
  `just board-log`; 1336 lines, OS ring 68256 bytes).
- Reference: QEMU `smp` lane (four harts), `build/logs/smp--2026-09-24T00-14-45/uart.log`.

## What the trace shows

| line | marker | note |
|---|---|---|
| 214 | `init: ready` | init's bootstrap done, the responder not yet running |
| 228–449 | 13 × `<svc>: ready` | blkd 267, socd 258, bundlemgrd 259, policyd 228, vfsd 431, keystored 437, execd 439, rngd 440, logd 442, samgrd 443, packagefsd 444, statefsd 446, bootctld 449 (torn) |
| 475 | `init: timing …` | the responder loop begins after this line |
| 553 → 558 | `inputd: ready` → `init: up inputd` | a frame sent AFTER the responder began arrives |
| 571 → 579 | `windowd: ready` → `init: up windowd` | same |
| — | `init: up <core svc>` | **none of the 13** |
| — | `stage: platform` | **never** |
| 883–884 | `FAIL stage wait svc=abilitymgr|sessiond stage=session-start` | the fleet waits on a fence that never moves |
| — | `FAIL ready announce` | none: every child's non-blocking send returned `Ok` |

On QEMU `smp` the same shape (ready lines 247–433 before `init: timing` 490) drains cleanly:
`init: up keystored` 491 … `stage: platform` 507.

## Hypotheses the next trace decides

The responder had two silent paths; both are now witnesses (init `responder.rs`, `diag.rs`):

1. **Frames arrive garbled** → `init: ctrl frame unknown svc=<s> len=<n> head=<16 hex>` (a
   cross-hart payload or ordering defect of the kernel's IPC on this SoC — the 13 senders run on
   harts 1–3 while init polls from hart 0).
2. **The channel is refused** → `init: ctrl recv err svc=<s> err=<label>` (the board's grant set
   differs from QEMU's — no virtio planes, an SDHCI host — so init's kernel-allocated slots
   differ; an alias with a slot closed after wiring would refuse exactly the channels created
   first).
3. **No frame at all** → neither witness: the frame was accepted by the child's send and never
   reached init's queue.

## Protocol

1. Flash the instrumented image (`just board-flash --plan build/board/bpi-f3/flash --yes`
   from download mode), let the LED ladder run out, microSD in, reset.
2. `just board-test PROFILE=board-headless --no-flash` (or `just board-log` and
   `scripts/board-test.sh --log=build/logs/latest-board/uart.log`).
3. Read the witnesses between `init: timing` and the first `init: up`.

## Results (cycle with the instrumented image dev-6bd69ea8, 2026-09-29 15:xx)

Neither witness fired. The frames DO arrive — the trace holds every `init: up <svc>` and
`stage: platform`, woven byte by byte into a `[USER-PF]` dump another hart printed at the same
time (the standing fault injector's child, `child: fault start`, faults on purpose every boot):

```
init: up keystored
[iUnSiEtR:- PuFp]  rLnOgAdD          ← "init: up rngd" ⨉ "[USER-PF] LOAD"
0i0n0i0t0:0 1u0p2 0p osltivcayld=0   ← "init: up policyd" ⨉ "…0000001020 stval=0"
PsFt]a gree:g sp laa2t=f0oxr0m0      ← "stage: platform" ⨉ "PF] regs a2=0x0…"
```

De-interleaved (`grep -E 's.?t.?a.?g.?e.?:.? .?p.?l.?a.?t.?f.?o.?r.?m'`): line 499 is
`stage: platform`. The same tearing is in the previous capture (`child: fault stainit: up
windowd`). So the hypotheses were wrong in the same way: nothing was lost in IPC; the CONSOLE
lost the lines. `sys_debug_write` holds the UART mutex for a whole write, but the kernel's
trap/fault/panic printers are lock-free by design (the mutex may be held on the faulting hart)
and write byte by byte through `console_write_byte` — on four real harts they interleave with
any other writer at byte granularity. QEMU's serialised emulation made this rare enough to
never fail a lane.

**Root cause:** no line-level ownership of the console across harts.

**Fix (kernel, `hal/console_line.rs` + pure `console_line.rs`, host-tested):** each hart
GATHERS its line in its own buffer and sends it at `\n` as one unit — ring and UART bytes, in
order — through a gate held only while pure console bytes go out. Nothing ever waits for a
hart that is doing anything else, so no kernel lock can couple with the console. (The first
cut — a hart OWNING the line from its first byte, others waiting — coupled exactly so on QEMU:
`KSELFTEST: ipc call budget FAIL (rt=275us budget=64)`, `statefsd: write budget exceeded
(ns=1.2e9)`, three attempts red. Measured, then replaced.) The gate's escapes: same-hart
re-entry writes through; a gate held past 30 ms is abandoned (a hart that died mid-unit) and
taken over; a non-holder's release is a no-op. The panic path flushes its own hart's partial
line first; a silent death costs at most one partial line per hart in the ring (RFC-0107 note).

**Proof (cycle with dev-755d5e41, `board-boot-2026-09-29T18-26-58-line-atomic.txt`):** 0 woven
lines, `stage: platform` at line 514, 36 `init: up` lines, `[PASS] board-headless: 17 rungs, no
FAIL marker, manifest clean`. QEMU `smp1`: green, ring = UART byte for byte (trace contract),
`ipc call budget ok`, `cpuid tp ok`. (QEMU `ci-os-smp`, MTTCG, is red on `ipc call budget FAIL`
since at least 2026-09-23 on every attempt, independent of this change, and is not in
`test-all`; its transcripts show 0 woven lines.) The line-atomic console also made
`SELFTEST: walltime rtc FAIL` whole for the first time — the board's RTC has no driver; tolerated
with its reference in `config/fail-marker-allow-board.txt`.

**Still red after this, separately:** `stage: display-ready` never opens on the board — gpud
exits (`init: service exit name=gpud reason=error`, no display device until D4), so the
DisplayReady barrier waits forever and abilitymgr/sessiond never start (`FAIL stage wait …
session-start`). That is the D-track's (TASK-0251) to close; the headless ladder ends at
`stage: platform`.
