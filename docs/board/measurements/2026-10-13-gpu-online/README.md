<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# 2026-10-13 — the GPU online: power-up, the published firmware's boot, the interrupt, a null job (TASK-0329 G1)

## Question

Does the board's GPU come online through our chain — the domain by its software sequence
through socd, the published firmware booted by the open start sequence, the interrupt served,
a null job's timeline write bridged to a kernel fence — and what does a null job's round trip
cost against the stock stack's fixed cost of a frame (0.64 ms, `../2026-10-10-gpu-stock-baseline/`)?

## Instrument

gpud's GPU backend lines in the board trace (`scripts/board-test.sh`, profile `board-headless`
or `-visible`): `socd: bring-up …gpu… ok (…)` with every APMU word it wrote; `gpud: gpu core ok
(bvnc= id=)`; `gpud: gpu fw ok (boot_us= irqs= fw_irqs=)` or its FAIL form with
`MIPS_EXCEPTION_STATUS`, `MARS_IDLE`, the boot-stage word and the fault registers; `gpud: gpu
mmu ok (map_us= unmap_us=)`; `gpud: gpu fence ok (rt_us= n=64)`; `gpud: gpu caps (…)` from the
firmware's device-info block. The stock system's live registers (`../2026-10-10-gpu-stock-baseline/
gpu-registers-running.txt`) are the expected values where our bring-up writes the same words.

## Hypotheses the first cycle decides

| # | Hypothesis | Decision line |
|---|---|---|
| H1 | Domain 2 comes up by the vendor's software sequence (sleep1, sleep2, isolation released; status `0x0f0` bit 0) | `socd … 0x0d0 … >0xe`, `0x0f0` bit 0 set; else a named fault with the status word |
| H2 | The register window answers once domain, clock and reset are right | `gpud: gpu core ok (bvnc=36.29.52.182 id=0x0024001d003400b6)`; all-ones = not powered or not clocked |
| H3 | The published firmware boots on this core with the open start sequence (untested upstream) | `gpud: gpu fw ok (boot_us=…)`; the FAIL form's registers tell a remap/boot-data fault (ours) from a core/firmware mismatch (theirs) |
| H4 | Line 75 reaches gpud; the firmware's `interrupt_count` matches ours | `irqs=N fw_irqs=N`; `irqs=0 fw_irqs>0` = the line (enable read back, hart) |
| H5 | Firmware-shared words are seen through explicit cache maintenance (no uncached user mapping exists yet: a clean after every CPU write, a flush before every CPU read of a firmware-written word) | the first `HEALTH_CHECK` return slot flips to EXECUTED within its bound (`health_us=` in the fw line); a slot that never flips while `fw_irqs` advances = a cache fault |
| H6 | The firmware processes a client command buffer holding ONE `UPDATE` (no task command) on a kick and writes the timeline word | `gpud: gpu fence ok (rt_us=…)`; `FAIL gpu fence (job 1 not done … cccb_read=0)` = the kick needs a task command (then the `NULL` task type, 210, is the next candidate) |

## Results

**Cycle 1, `board--2026-10-13T11-48-16`** (image with the first loader): H1 and H2 decided, the
loader stopped before the firmware for a fault of ours.

| line | reading |
|---|---|
| `socd: bring-up /soc/multimedia-bus/gpu@cac00000 ok (domains=1 resets=1 clocks=1 rates=1 … writes=7) apmu+d0:0>e apmu+f0:8008>8009 apmu+cc:1000>12` | **H1 confirmed**: the software sequence brought domain 2 up (control `0 → 0xe`, status bit 0 set), the clock gated on, the reset released, the rate word at the stock `0x12` (the boot loader had left `0x1000`: divider bit 12 set, gate off, reset held) |
| `gpud: gpu core ok (bvnc=36.29.52.182 id=0x0024001d003400b6)` | **H2 confirmed**: the window answers with the measured id — domain, clock and reset are right |
| `gpud: FAIL gpu firmware (connection ctl)` | ours: the heap map reserves the three config slots at creation; the loader reserved them again (`Overlap`). Fixed: the slots are backed, not re-reserved |

**Cycle 2, `board--2026-10-13T11-59-42`**: H3 and H4 decided; the health check's oracle was ours,
not the interface's.

| line | reading |
|---|---|
| `gpud: FAIL gpu fw (started in 1080us but the health check got no return in 1000083us irqs=1 fw_irqs=1 executed=1 pow=0x3 hwr=0x1)` | **H3 confirmed**: the published firmware boots on this core with the open start sequence — `firmware_started` after 1 080 µs (load included). **H4 confirmed**: one interrupt reached gpud, the firmware counted one. The health check WAS executed (`kccb_cmds_executed` 0 → 1; power state IDLE, hardware OK) — the firmware answers a health check without writing its return slot, as the open driver's own use of it (sent without a slot, liveness read from `kccb_cmds_executed`) implies. The rung now reads that counter; the return slot stays in the line as a witness (`rtn=`). H5 moves to the MMU-cache flush, whose return slot the open driver does wait for |

**Cycle 3, `board--2026-10-13T12-12-45`**: H5 decided; H6 answered with a reason.

| line | reading |
|---|---|
| `gpud: gpu fw ok (boot_us=1092 health_us=82 irqs=1 fw_irqs=1 executed=1 rtn=0x0 stage=0 ts=2406 pow=0x3 hwr=0x1 …)` | the firmware up in 1.09 ms, the health check executed in 82 µs; `rtn=0` is the witness that the firmware answers a health check without its return slot |
| `gpud: gpu caps (va_bits=40 page=4096 clusters=1 … cache_block=64 backing_free=61 …)` | the harts' cache block is 64 bytes; 67 of 128 backing pages were used by the firmware's load |
| `gpud: gpu mmu ok (map_us=65 unmap_us=50 pc=0x5743000 map_flags=0x400001f unmap_flags=0x400001f table_pages=1)` | **H5 confirmed**: the MMU-cache flush's return slot flips through the cache maintenance; a bind plus its flush round trip costs 65 µs, the unbind 50 µs |
| `gpud: FAIL gpu fence (job 1 not done in 1000059us word=0 cccb_read=0 woff=48 irqs=4 pow=0x3 hwr=0x1)` then `gpud: gpu fwccb (type=112)` | **H6: no, with a reason**: message 112 is the firmware's page-fault notification — the kick made the firmware touch an address its page table did not hold; the client buffer was never read (`cccb_read=0`). The open driver flushes the MMU cache between mapping firmware objects and the kick that reaches them (`pvr_mmu_flush_exec` before any job); the MIPS TLB holds page pairs, so a neighbour page that was unmapped when its pair was loaded stays invalid until that flush. Cycle 3 kicked the freshly mapped context without one. Cycle 4 adds the flush and decodes the fault address |

**Cycle 4, `board--2026-10-13T13-02-42`** (the MMU-cache flush between mapping the context and
the kick; the fault address decoded): `gpu fw ok (boot_us=23227 health_us=135 …)` — the
firmware's boot time varies (1.1 ms in cycle 3, 23 ms here; both far inside the 5 s budget; the
cause is the next cycle's measurement: the firmware's own `firmware_started_timestamp`
`ts=96440` against `ts=2406`); `gpu mmu ok (map_us=57 unmap_us=53)`; `FAIL gpu fence (job 1 not
done … word=0 cccb_read=0 … fwctx=0xc002b000 cccb=0xc0043000 ctl=0xc002c000 ufo=0xc002e000)` with
**no page-fault message any more** — the flush removed the fault, and the firmware still reads
nothing of the client buffer. **H6 falsified: a kick whose client buffer holds an `UPDATE` alone
is not processed.** A job in the interface is a TASK command plus its update; cycle 5 writes the
interface's `NULL` task (type 210) before the `UPDATE` — the null job proper (H6b).

Beside the GPU: the QEMU `visible` lane went red with this image's gpud (`init: service exit
name=gpud reason=fault`, a store page fault just under the stack pointer): the GPU state (a
fixed table of 64 page-table slots, 6.5 KiB) sat in the service loop's static stack frame on
every machine, QEMU's included, and gpud's 32 KiB stack overflowed. The slot tables moved to the
heap, the state is boxed, the ladder is never inlined into the loop.

**Cycle 5, `board--2026-10-13T13-46-51`** (the `NULL` task before the `UPDATE`; the GPU state on
the heap): the whole ladder.

| line | reading |
|---|---|
| `gpud: gpu fw ok (boot_us=10862 health_us=82 … ts=53519 …)` | boot 10.9 ms this time (1.1 / 23 / 10.9 ms over three boots — open, below) |
| `gpud: gpu mmu ok (map_us=57 unmap_us=50 …)` | as in cycles 3 and 4 |
| `gpud: gpu fence ok (rt_us=131 worst_us=3190 n=64 irqs=64 below the stock 640us)` | **H6b confirmed**: a job is a TASK command plus its update — with the interface's `NULL` task the firmware processes the client buffer, writes the timeline word and raises one interrupt per job (64 of 64); gpud signals the kernel fence to it. The mean round trip — submit, kick, firmware, interrupt, read, `fence_signal`, `fence_wait` — is **131 µs**, the stock stack's fixed cost of a frame 640 µs: 4.9× below the bar. The worst of 64 is 3.2 ms (one job; the first, or one that met the console) |
| judge | `ladder complete: 55 rungs present`, no FAIL marker; the operator rungs decide the `[PASS]` |

Beside it, the same boot: `KSELFTEST: ipc call budget ok (rt=52us budget=64)` — the 97 µs of
2026-10-13 morning did not recur (S3's placement finding is intermittent, as read), `bkl budget
ok (max_wait=624us max_hold=0ms)`, `console off lock ok (max_us=198)`.

**Open, measured, handed on:** the firmware's boot time varies tenfold between boots (1.1, 23,
10.9 ms; its own `firmware_started_timestamp` 2406 / 96440 / 53519 ticks) — far inside the 5 s
budget, but a frame's start-up is layer 4's business: measure against the firmware trace
(groups MAIN|POW) in G2's first cycle.

CPU presents across the cycles (`gpud: dc present us avg=`): the first eight windows sit at
~110 ms in every cycle, with and without the GPU path (cycle 08-28 had none) — the running
firmware does not slow the CPU composite. The late windows of 300–600 ms appear in cycles 2 and
3 alike at the large layouts (the operator's "slower, some flicker") and belong to the frame
arena's spill (the memory lane's input, TASK-0290), not to the GPU path.

Operator observation, cycle 2's first boot (the trace holds both boots): after the theme switch
to light (`uitheme: switched (to=light)`) hidrawd reports `inputd send fail backpressure` and
`chain I2 wire send FAIL (inputd route)`, inputd's push rate falls to zero and the desktop no
longer takes input; the second boot ran every rung. The GPU path had ended seconds earlier
(its FAIL line is in the first boot too) — the stall is the input/windowd chain's, owner noted
in TASK-0329's hand-ons (the memory lane's spill findings sit in the same minute).

## Verdict

- **The GPU is online through our chain** (cycle 5): domain 2 by its software sequence, the
  published firmware booted by the open sequence, the interrupt served, one address space
  bound and flushed, a null job's completion bridged to the kernel timeline fence.
- **A null job's round trip costs 131 µs** against the stock stack's 640 µs fixed cost of a
  frame — the part a driver and a system own is 4.9× below the bar; the per-pixel costs stay
  the silicon's (layer 4 measures them).
- Five cycles, five decisions, each from a line: a double reservation of ours (1); the
  health check's return slot is not the firmware's liveness (2 — the open driver's
  `kccb_cmds_executed` is); the MMU-cache flush between mapping and kick is mandatory (3, a
  firmware page fault without it); an `UPDATE` alone is no job (4); a `NULL` task plus its
  update is (5). The gate was red on every real fault before it was green.
- Two findings for the lanes above: firmware-shared memory must be mapped uncached for several
  commands in flight (the memory lane, TASK-0290 input); the firmware's boot time varies
  tenfold between boots (layer 4's first cycle, with the firmware trace).
- The CPU composite is untouched by the running firmware (the early present windows are the
  same ~110 ms with and without it); the late 300–600 ms windows are the frame arena's spill.
