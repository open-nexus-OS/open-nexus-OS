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

## Results

_pending the first cycle with the instrumented image._
