<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# 2026-11-10 — the SMP baseline before the kernel lock is taken apart (TASK-0330 P0)

Status: **instrument built, hypotheses written; the board boot and the verdicts are open.**

## Question

Before the lane changes the kernel: where does the kernel lock's time go, how long does a
runnable task wait for a hart, what do the harts offer, and how fast do they run? The order in
which the lock is decomposed (RFC-0102) follows the first answer; the scheduler's goal
(TASK-0363) is judged against the second; the userspace baseline (TASK-0362) follows the third.

## Instrument

All of it prints once per hart at boot or once at the boot-end fence, never a line per event.

| Line | From | Says |
|---|---|---|
| `KINIT: bkl held us (window_ms= sched= ipc= mm= task= caps= misc= timer= ipi= ext= fault= idle=)` | `source/kernel/neuron/src/core/trap/lock_share.rs`, `baseline.rs` | microseconds the lock was held in the steady window, summed per group of holders — syscalls by number, and the trap kinds that are no syscall |
| `KINIT: bkl takes (…)` | same | how often each group took it |
| `KINIT: bkl top sum id=A:us/n id=B:us/n id=C:us/n` | same | the three holders with the largest sum; an id below 80 is a syscall number, 80 timer, 81 IPI, 82 external interrupt, 83 fault, 84 the scheduling loop, 85 other |
| `KINIT: ready latency class= n= mean_us= max_us= le100us= le1ms= le10ms= gt10ms=` | `source/kernel/neuron/src/sched/ready_stats.rs` | per QoS class: how long a task waited between entering a run queue and its dispatch |
| `KINIT: wake ipi total max= mean= n=` | `core/trap/budgets.rs` | the cost of a resched IPI from its send to its trap, whole boot |
| `KINIT: isa from hart= base= f= d= v= zfh= zvfh= zba= zbb= zbs= zicbom= zicboz= svpbmt= sscofpmf=` | `source/kernel/neuron/src/core/boot_fdt.rs` | what the tree states per run of harts stating the same |
| `KINIT: cpuN speed (loop_us= iters=1000000 left=0)` | `source/kernel/neuron/src/core/cpu_main.rs` | the time of one fixed loop of two instructions on that hart |
| `KGATE: smp bringup …`, `KSELFTEST: bkl budget …`, `KINIT: bkl hold histogram …` | as before | harts online, the longest waits and holds |

The stock side of the speed line: `hartspeed.rs` in this folder — the same two instructions
(`addi`, `bnez`, one million times) as a static program without a C library, run from the stock
system's `/tmp`. It pins itself to each CPU in turn and prints one line per CPU
(`cpuN loop_us= min_us= iters= left=`; `loop_us` is the first run, as the kernel line is one
run, `min_us` the fastest of five). The ratio of the two times is the ratio of the clocks. Its
build line is in its header; it was run under the user-mode emulator before the board.

## Hypotheses (written before the boot)

| # | Hypothesis | Decision criterion |
|---|---|---|
| H1 | IPC holds the largest share of the lock in the steady window. | `ipc` is the largest of the syscall groups in `bkl held us` → the IPC objects leave the lock right after the run queues; else the largest group goes first and the ADR says why. |
| H2 | The scheduling loop (`idle`) and the timer are a visible share: every hart's pick takes the global lock. | `idle + timer` above 20 % of the held time → the run queues leave first (TASK-0363 P2 before TASK-0330 P2). |
| H3 | Faults are a small share today because one thread touches memory at a time. | `fault` below 10 % → address spaces leave last (TASK-0330 P3), as planned. |
| H4 | The interactive class waits within interrupt latency since ADR-0096; the normal class waits up to a tick. | `class=interactive` `le1ms` at least 99 % of `n`; `class=normal` shows a `le10ms` tail. Otherwise the wake path has a fault to find before the scheduler is rebuilt. |
| H5 | The harts run at the stock system's clock. | `loop_us` within 10 % of the stock program's on both clusters. Slower → the clock setup is part of TASK-0330 P1 (socd), and every CPU number of this lane is re-read after it. |
| H6 | The tree states F, D and V on every hart; the two clusters differ only outside these. | `f=1 d=1 v=1` on every `isa` line → one userspace baseline for all harts (TASK-0362); else the baseline is the intersection. |
| H7 (added after the QEMU lanes, before the board) | The walk over every capability table (below) is the largest holder on the board too. | `id=18` and `id=20` among the three holders of `bkl top sum`, each per take at least five times the group's other syscalls → TASK-0330 P5 runs before the split order is written, and the order is read from the boot after it. Not among them → the order is written from this boot; P5 still runs before the IPC objects leave the lock (RFC-0102 forbids the walk). |
| H8 (the instrument itself) | The instrument does not change what it measures. | `KSELFTEST: ipc call budget` within the board's earlier band (rt 50–51 µs, mean 61–63 µs, 2026-11-10). Above it → the clock read on this SoC is a trap, and the instrument becomes a boot knob before any number of it is used. |

## QEMU (the instrument's own proof)

QEMU proves that the instrument prints and that the ladder stays green with it. Its times are
the emulator's (one hart under instruction counting), not the board's: no hypothesis is judged
here.

### One hart (`just ci-os-smp1`, `build/logs/smp1--2026-11-10T17-43-51`, ladder green, 13/13 chain markers)

```text
KINIT: isa from hart=0 base=rv64i f=1 d=1 v=1 zfh=1 zvfh=1 zba=1 zbb=1 zbs=1 zicbom=1 zicboz=1 svpbmt=1 sscofpmf=1
KINIT: cpu0 speed (loop_us=4000 iters=1000000 left=0)
KINIT: bkl held us (window_ms=5099 sched=1589164 ipc=1902315 mm=14187 task=7622 caps=530909 misc=25588 timer=29522 ipi=0 ext=1480 fault=52023 idle=32026)
KINIT: bkl takes (sched=415005 ipc=33514 mm=323 task=32 caps=5029 misc=4693 timer=1734 ipi=0 ext=379 fault=8401 idle=4026)
KINIT: bkl top sum id=18:1727727us/11372 id=0:1577228us/414820 id=20:519638us/2184
KINIT: ready latency class=normal n=425081 mean_us=6 max_us=4986 le100us=419458 le1ms=5207 le10ms=416 gt10ms=0
KINIT: wake ipi total max=0us mean=0us n=0
```

What the lane says about the instrument, not about the system:

- The groups sum to 4.18 s of a 5.10 s window: on one hart the kernel is entered for most of
  the window, because the selftest's quiet window is a yield loop (`id=0`, 414 820 yields,
  3.8 µs each — the `sched` group is that loop, and the `normal` class's 425 081 dispatches are
  its requeues). The board's window has the desktop in it instead.
- Only `class=normal` is printed: a class without a dispatch in the window prints no line. In
  this lane's window one task wakes (`sweep steady … wakes=1`), so the interactive chain has
  nothing to report here; the visible lanes and the board do.
- The instrument costs nothing an IPC call shows: `KSELFTEST: ipc call budget ok (rt=47us
  mean=52us)` against `rt=47us mean=51us` on the same lane before it
  (`build/logs/smp1--2026-11-10T15-58-36`); the yield loop ran 5 % fewer rounds in its window
  (420 628 against 444 240). The board's `ipc call budget` line is read against its earlier
  band (rt 50–51 µs, mean 61–63 µs on 2026-11-10) for the same check.

### The first finding: a receive and a close walk every capability table

The two largest holders are a receive (`id=18`, 152 µs per take) and a capability close
(`id=20`, 238 µs per take) — at this lane's 2 ns per instruction, 76 000 and 119 000
instructions each, on a lane whose whole IPC call round trip is 47 µs. The cause is in the
code, not in the emulator: a blocking receive on an empty queue asks whether a foreign sender
remains (`syscall/api/eof_scan.rs`, `foreign_sender_remains`), and a close of an endpoint
capability asks who still names the endpoint (`census`); both walk every capability slot of
every task under the kernel lock (98 tasks in this window). The remaining IPC syscalls cost
7.9 µs per take, the remaining capability syscalls 4.0 µs.

Consequence (TASK-0330 P5, RFC-0102 "no path walks another process's table"): an endpoint
counts the capabilities that name it, the walk is deleted, and the split order is decided from
a measurement taken after it — otherwise H1 would be read from a scan, not from IPC.

### Two harts (`just ci-os-smp`, `build/logs/smp--2026-11-10T17-57-32`, multi-thread emulation)

```text
KGATE: smp bringup ok mask=0x3
KINIT: cpu0 speed (loop_us=5814 iters=1000000 left=0)
KINIT: cpu1 speed (loop_us=5787 iters=1000000 left=0)
KINIT: bkl held us (window_ms=11266 sched=672443 ipc=6577455 mm=77492 task=30787 caps=1032285 misc=117051 timer=340906 ipi=0 ext=7535 fault=252251 idle=644310)
KINIT: bkl takes (sched=75849 ipc=40730 mm=1093 task=32 caps=4216 misc=4699 timer=4129 ipi=0 ext=181 fault=8401 idle=20950)
KINIT: bkl top sum id=18:5020069us/15235 id=20:990931us/1778 id=84:644310us/20950
KINIT: ready latency class=normal n=88518 mean_us=139 max_us=10490 le100us=79905 le1ms=4362 le10ms=4247 gt10ms=4
KINIT: wake ipi total max=2742us mean=11us n=17295
KSELFTEST: bkl budget ok (max_wait=3513us max_hold=3ms nr=11)
```

- The lines print on both harts' data; the lock is held 9.75 s of an 11.27 s window, 5.02 s of
  it by the receive's walk and 0.99 s by the close's.
- `ipi=0`: a resched IPI takes the lock only when it interrupts user mode; here all 17 295
  reached a hart in its scheduling loop, which is the `idle` group's 20 950 takes.
- The lane itself is red, as it has been since at least 2026-10-20
  (`build/logs/smp--2026-10-20T11-00-48`, `ipc call budget FAIL (rt=259us budget=64)`): it
  judges the IPC call budget, a board value, under multi-thread emulation — today
  `rt=525–543us` in all three attempts, once joined by `bkl budget FAIL max_wait=40450us` and
  once by `console off lock FAIL max_us=1069`. Whether today's higher value is three weeks of
  changes or the walk above under two harts, this lane cannot say; TASK-0330 P1's gate asks
  for this lane green, and P5 removes the walk first.
- These boots ran before the later phases of the four phased syscalls and the cache-right trap
  were named (they counted as `misc`); since then every take names its holder. The one-hart
  lane repeated with that build is green and shows the same picture
  (`build/logs/smp1--2026-11-10T18-30-11`: `id=18:1732950us/11402`, `id=20:528521us/2219`,
  `ipc call budget ok (rt=47us mean=56us)`).

### Eight harts in the tree, ten boots (`SMP=8` on the two-hart profile, a local override)

```text
nxboot: fdt ok (harts=8 …)
KGATE: smp hart1 online=1 asm=1 stage=4 hsm=0
KGATE: smp hart2 online=1 asm=1 stage=4 hsm=0
KGATE: smp hart3 online=1 asm=1 stage=4 hsm=0
KGATE: smp bringup ok mask=0xf
```

Ten of ten boots (`build/logs/smp--2026-11-10T18-03-45` … `T18-27-24`): the kernel starts the
four harts its constant allows of the eight the tree names, and loses none of the four
(`mask=0xf`, no panic). The four it does not start are not named on the console — TASK-0330
P1's gate is 0 lost of eight, ten times.

## Inventory: what the hart-count constant sizes today (counted 2026-11-10)

`MAX_CPUS = 4` has 95 uses in 17 kernel files (`grep -rn MAX_CPUS source/kernel/neuron/src`):
`core/smp/mod.rs` 22, `core/smp/runtime.rs` 13, `sched/mod.rs` 10, `core/smp/tlb.rs` 8,
`core/smp/bringup.rs` 7, `selftest/mod.rs` 6, `task/mod.rs` 5, `core/cpu_main.rs` 5,
`core/irq.rs` 4, `hal/console_line.rs` 3, `core/trap/runtime.rs` 3, `hal/platform.rs` 2,
`core/smp/lock_ping.rs` 2, `core/kmain.rs` 2, and one each in `task/affinity.rs`,
`mm/vmo/creating.rs`, `hal/plic.rs`.

| Sized by it | Where |
|---|---|
| 21 statics | `HART_LOCALS`, `RESCHED_PENDING`, `RESCHED_IPI_SKIPPED`, `RESCHED_REQ_ACCEPTED`, `RESCHED_IPI_SENT_OK`, `RESCHED_SSOFT_TRAPS`, `RESCHED_ACK` (`core/smp/mod.rs`); `USER_DISPATCHES`, `LAST_STEAL_NS`, `TIMER_TICKS`, `WAKE_HINT` (`core/smp/runtime.rs`); `TLB_MAIL`, `HART_ACTIVITY` (`core/smp/tlb.rs`); `BRINGUP_ASM_SEEN`, `BRINGUP_STAGE` (`core/smp/bringup.rs`); `SCHED_LOOP_ENTERED`, `EXEC_PROOF_EMITTED` (`core/cpu_main.rs`); `ARMED_DEADLINE_NS` (`core/trap/runtime.rs`); `STASHED` (`core/irq.rs`); `PLIC_SCTX` (`hal/platform.rs`); `LINES` (`hal/console_line.rs`) |
| 2 tables inside a structure | the scheduler's `cpus` (`sched/mod.rs`), the task table's `current` (`task/mod.rs`) |
| 1 derived count | `CREATING_SLOTS = MAX_CPUS * 2` (`mm/vmo/creating.rs`) |
| loops and bounds | `task/affinity.rs`, `hal/plic.rs`, `core/kmain.rs`, `core/smp/lock_ping.rs`, the selftests' offline-CPU cases |

Outside the kernel a hart is named by literal in `source/init/nexus-init/src/affinity.rs`
(`0b0001`, `0b1111`, `0b1110`), `bootstrap/resume.rs` (`AFFINITY_ANY = 0b1111`),
`source/services/execd/src/sched_recipe.rs` (`0xF`, twice), `PINCHED_WORKERS`, and the
workpool's `MAX_WORKERS = 4`. TASK-0330 P1 (RFC-0118) replaces all of it by the tree's count.

## Board (`build/logs/board--2026-11-11T09-05-45`, build `dev-95cbb354` of `9992d453`, the visible profile with the desktop used)

```text
KSELFTEST: platform from fdt ok (… tb=24000000Hz harts=8 banks=2 boot_hart=0 timer=sstc …)
KINIT: isa from hart=0 base=rv64i f=1 d=1 v=1 zfh=0 zvfh=0 zba=1 zbb=1 zbs=1 zicbom=1 zicboz=1 svpbmt=1 sscofpmf=1
KGATE: smp bringup ok mask=0xf
KINIT: cpu0 speed (loop_us=803 iters=1000000 left=0)        # cpu1, cpu2, cpu3: 803 as well
KSELFTEST: ipc call budget ok (rt=25us mean=29us budget=64)
KINIT: bkl histogram le100us=1050895 le1ms=49543 le10ms=1366 gt10ms=0
KINIT: bkl hold histogram le100us=984108 le1ms=26346 le10ms=371 gt10ms=0 top nr=11:2097us nr=20:1734us nr=60:1733us
KINIT: bkl held us (window_ms=8941 sched=1051799 ipc=5489970 mm=906479 task=13125 caps=433253 misc=79194 timer=163368 ipi=0 ext=51803 fault=70844 idle=288397)
KINIT: bkl takes (sched=628294 ipc=83500 mm=287336 task=58 caps=4537 misc=7196 timer=5863 ipi=0 ext=5131 fault=22314 idle=57576)
KINIT: bkl top sum id=18:4953095us/34221 id=0:974467us/621998 id=60:550652us/376
KINIT: ready latency class=normal n=633555 mean_us=20 max_us=534002 le100us=628002 le1ms=3342 le10ms=2100 gt10ms=111
KINIT: ready latency class=interactive n=5380 mean_us=1513 max_us=10010 le100us=1055 le1ms=1358 le10ms=2966 gt10ms=1
KINIT: wake ipi total max=27us mean=5us n=7077
KSELFTEST: bkl budget ok (max_wait=6930us max_hold=2ms nr=11)
```

The stock system, the same loop (`hartspeed`, every CPU at its highest clock, 1.6 GHz):

```text
cpu0 loop_us=805 min_us=803 iters=1000000 left=0            # cpu1 … cpu7: min_us=803 as well
```

### What the lock's time says

The lock is held 8.55 s of the 8.94 s window (96 %) by four harts.

| Group | Held | Share | Takes | Per take | The largest in it |
|---|---|---|---|---|---|
| `ipc` | 5.49 s | 64.2 % | 83 500 | 65.7 µs | receive `id=18`: 4.95 s, 144.7 µs per take; the others 10.9 µs |
| `sched` | 1.05 s | 12.3 % | 628 294 | 1.7 µs | yield `id=0`: 0.97 s in 621 998 takes (70 000 a second) |
| `mm` | 0.91 s | 10.6 % | 287 336 | 3.2 µs | `id=60` (a memory object's runs for a device): 0.55 s in 376 takes, 1.46 ms per take; the others 1.2 µs |
| `caps` | 0.43 s | 5.1 % | 4 537 | 95.5 µs | close `id=20` (longest single hold 1.73 ms) |
| `idle` | 0.29 s | 3.4 % | 57 576 | 5.0 µs | |
| `timer` | 0.16 s | 1.9 % | 5 863 | 27.9 µs | |
| `misc` | 0.08 s | 0.9 % | 7 196 | 11.0 µs | |
| `fault` | 0.07 s | 0.8 % | 22 314 | 3.2 µs | |
| `ext` | 0.05 s | 0.6 % | 5 131 | 10.1 µs | |
| `task` | 0.01 s | 0.2 % | 58 | 226 µs | exit `id=11` (longest single hold 2.10 ms) |

- **The receive's walk is 58 % of the lock's time.** A receive on an empty queue holds the lock
  144.7 µs on the board (152 µs on the one-hart lane), thirteen times the group's other
  syscalls. Were it as short as they are, the lock would be held 3.97 s of the window (44 %).
- **A device asking for a lazy object's runs commits its pages under the lock**
  (`syscall/api/vmo_runs.rs`, `mm::vmo::named_runs`): 1.46 ms per call, 42 calls a second while
  windows open — the third largest holder. RFC-0102's rule for work whose length the caller
  decides (staged under the lock, done outside it) applies; it is built with the memory objects
  leaving the lock (TASK-0330 P3).
- **Something yields 70 000 times a second.** On the one-hart lane the count per second is the
  same (81 000) and the lane's quiet window is the cause named there; on the board the
  instrument does not say which task it is. The loop occupies a hart and takes the lock for
  11 % of its held time. The yielder is named before the run queues are judged (a count per
  task at the fence, with the next build of the instrument).
- **An interactive task waits a millisecond and a half for a hart.** 55 % of the interactive
  class's dispatches waited between 1 ms and 10 ms, the mean is 1.5 ms, the longest 10 ms (one
  tick); a frame that crosses five services of the display chain pays this five times. A
  normal task waited up to 534 ms. Whether an interactive wake waits behind a peer of its class
  on the one hart the chain is pinned to, or behind a lower class it should have preempted, the
  instrument does not yet say.

### Verdicts

| # | Verdict | Read from | Consequence |
|---|---|---|---|
| H1 | holds, but not as IPC | `ipc` is the largest group (64 %); nine tenths of it is the walk of one syscall | the order is read from the boot after TASK-0330 P5, as H7 demands |
| H2 | refuted | `idle + timer` = 5.3 % of the held time | the run queues do not leave first on this criterion; the `sched` group's 12 % is the yield loop, judged once its task is named |
| H3 | holds | `fault` = 0.8 % | address spaces leave last (TASK-0330 P3) |
| H4 | refuted for the interactive class | `le1ms` holds 44.9 % of `n`, not 99 %; the normal class shows its tail (`le10ms`=2100, `gt10ms`=111) | the wake path is examined before the scheduler is rebuilt: the wait is attributed (peer of the class on the pinned hart, or a missed preemption) in TASK-0363 P0 |
| H5 | holds | 803 µs on our four harts, 803 µs on all eight CPUs of the stock system at 1.6 GHz | the clock setup is not part of TASK-0330 P1; the CPU numbers of this lane stand |
| H6 | holds | one `isa` line for all eight harts: `f=1 d=1 v=1` (`zfh=0 zvfh=0`: no half precision) | one userspace baseline with F, D and V (TASK-0362) |
| H7 | holds for the receive, not for the close | `id=18` is the largest holder, 13 times its group's others; `id=20` is not among the three (the `caps` group holds 0.43 s, behind `id=60`) | TASK-0330 P5 runs before the split order is written |
| H8 | holds; the band was too narrow | `rt=25us mean=29us`. The board's earlier boots span rt 24–50 µs (2026-11-05: 24/60 and 49/62 as rt/mean; 2026-11-09: 40/456; 2026-11-10: 50/61) — two modes of `rt`, 24–25 and 49–50, not one band | the instrument stays always on; which hart serves the call decides the mode, which is why TASK-0330 P2's gate measures the call on every hart |

The gates this boot sets: TASK-0330 P5 on the board — `ipc` at most 16.4 µs and `caps` at most
23.9 µs per take (a quarter of 65.7 and 95.5). The split order is written as an ADR from the
boot after P5.

## Board after TASK-0330 P5 (`build/logs/board--2026-11-11T10-01-24`, build `dev-ef48c745`, the visible profile)

The tree: a held capability counted at its object (TASK-0330 P5) and the instrument of
TASK-0363 P0 (`late_idle`/`late_lower`/`late_peer` on the ready-latency line, `KINIT: yield
top`). The ladder is complete (106 rungs); the sight acks are the operator's.

```
KGATE: smp bringup ok mask=0xf
KSELFTEST: ipc call budget ok (rt=51us mean=65us budget=64)
KINIT: bkl held us (window_ms=3500 sched=2042360 ipc=150200 mm=565376 task=4361 caps=12675 misc=22973 timer=10708 ipi=0 ext=17626 fault=20804 idle=228639)
KINIT: bkl takes (sched=1095978 ipc=39571 mm=402410 task=47 caps=5207 misc=4521 timer=1129 ipi=0 ext=2498 fault=8298 idle=51449)
KINIT: bkl top sum id=0:2028553us/1090420 id=55:556199us/400466 id=84:228639us/51449
KINIT: bkl histogram le100us=1610296 le1ms=811 le10ms=0 gt10ms=0
KINIT: bkl hold histogram le100us=1547645 le1ms=45 le10ms=0 gt10ms=0 top nr=11:234us nr=16:204us nr=44:170us
KINIT: ready latency class=normal n=1098576 mean_us=0 max_us=1640 le100us=1098333 le1ms=241 le10ms=2 gt10ms=0 late_idle=0 late_lower=0 late_peer=2
KINIT: yield top total=1090420 pid=63:0x52c6c4a34ffb3f69:689888 pid=71:0xb64b904a0313921c:400465 pid=102:0x0:64
KINIT: wake ipi total max=15us mean=1us n=8141
KSELFTEST: bkl budget ok (max_wait=556us max_hold=0ms nr=11)
```

| | before P5 (`09-05-45`) | after P5 (`10-01-24`) | gate |
|---|---|---|---|
| `ipc` per take | 65.7 µs | 3.8 µs (150 200 µs / 39 571) | at most 16.4 µs — met |
| `caps` per take | 95.5 µs | 2.4 µs (12 675 µs / 5 207) | at most 23.9 µs — met |
| `ipc` share of the held time | 64.2 % | 4.9 % | |
| longest wait for the lock | 6 930 µs | 556 µs | |
| waits above 1 ms | 1 366 | 0 | |
| longest hold | 2 097 µs | 234 µs | |
| holds above 1 ms | 371 | 0 | |

### What this window is, and what it is not

- **The window is not the one of the boot before.** It is 3.5 s long and ends before the
  operator's input: `SELFTEST: ui v2 input ok` follows the fence here and preceded it there. No
  task of the interactive class was dispatched in it (the line is absent), so the attribution
  TASK-0363 P0 asks for — why an interactive wake waits above one millisecond — is **not read
  from this boot**. The per-take costs compare (a take costs what it touches now, whatever the
  system holds); the shares and the ready latency do not.
- **Two polling loops hold the lock now**: 66 % of the held time is `yield` (`id=0`) and 18 %
  is `mmio_map_auto` (`id=55`). `yield top` names them: the ladder's own client
  (`0x52c6…3f69`, 689 888 yields) and the network stack (`0xb64b…921c`, 400 465 yields — one per
  `mmio_map_auto`, 400 466 takes). The board has no network device behind capability slot 48;
  the network stack asks for it 114 000 times a second for twenty seconds
  (`source/services/netstackd/src/os/bootstrap.rs`: a retry with a `yield` until a deadline),
  then states `netstackd: mmio cap48 missing` — and its halt is a `yield` loop again, for as
  long as the board runs. On the emulator, where the device exists, it yields 166 times.
- **The yield loop of the baseline is named**: 70 000 yields a second there were these two
  tasks under a lock held 96 % of the time; freed of the walks they reach 311 000 a second.

### What follows

- TASK-0330 P5's numbers hold on the board. The split order (the ADR) is not written from this
  boot: its largest holders are two loops that are not the system's work. It is written from
  a window that contains the desktop in use, after the network stack's probe is one question
  and not a loop.
- TASK-0363 P0: the yielders are named; the interactive attribution needs a window that
  contains interactive work — the baseline lines printed a second time when the operator's
  input window closes.

## Board with the device planes decided by the tree (`build/logs/board--2026-11-11T10-41-29`, build `dev-dd1c4b5a`)

`init: withheld svc=netstackd|ingressd|dsoftbusd (no net plane)` — the three services standing
on the network plane stay suspended on the board. The fence window (3.66 s):

| | before (`…T10-01-24`) | now |
|---|---|---|
| `bkl held us mm=` | 565 376 (18 %) | 11 254 (0.4 %) |
| `bkl takes mm=` | 402 410 | 2 880 |
| `yield top`, the network owner | 400 465 | not among the three |
| `yield top`, the proof harness | 689 888 | 1 467 706 |
| longest lock wait | 556 us | 261 us |
| `ipc call budget` rt | 51 us | 34 us |

The harness's ladder is now the one yielder of the fence window (its RPC waits); it takes the
time the network owner's loop left.

**Not read in this cycle:** the windows after `SELFTEST: input window open`. The harness never
reached that line: its supervised-restart proof pinged an instance that was already dying (the
route ask overtook init's notice of the exit while a login was in progress) and then waited on
a receive nothing could end. The proof is rewritten on the exchange that ends by the answer
or the peer's death (no yield, no deadline; one-hart lane `build/logs/smp1--2026-11-11T10-52-55`).

**Red in this cycle:** `xhcid: FAIL (step=control cc=4)` — one device behind the hub (the hub's
own billboard function, enumerated in every earlier cycle) ended a control transfer with a
transaction error and was given up; keyboard and mouse enumerated and reported. Seen once;
judged at the next cycle (recurs at the same step: a timing fault of ours; does not: the bus).

## The desktop in use, ten windows of 10 s (`build/logs/board--2026-11-11T11-05-00`, build `dev-4c9ca759`)

The windows the split order is written from (ADR-0098). The harness reached
`SELFTEST: input window open`; its observer printed the baseline every 10 s while the desktop
was used (pointer, keyboard, apps opened and closed). Sums over the ten windows:

| `bkl held us` | held | takes | per take |
|---|---|---|---|
| `idle` | 39 184 957 | 9 117 091 | 4.3 us |
| `ipc` | 10 625 613 | 3 234 308 | 3.3 us |
| `mm` | 2 111 387 | 3 555 | 594 us |
| `misc` | 287 619 | 11 349 | |
| `fault` | 126 914 | 62 218 | |
| `sched` | 102 413 | 37 067 | 2.8 us |
| `ext` / `caps` / `timer` / `task` | 59 998 / 35 884 / 23 453 / 1 129 | | |

Top holders per window: the idle loop, `waitset_wait` (id 40: 231–581 ms), `ipc_send` (id 14:
233–497 ms), and in five windows `vmo_runs` (id 60: 234–795 ms in 196–903 takes, 0.4–1.5 ms per
take). `yield top total=0` in every window: nothing yields while the desktop is in use.

`ready latency class=interactive`, summed: n=273 295, le100us=270 578, le1ms=2 198,
le10ms=494, gt10ms=25, max 222 982 us; `late_peer=508 late_idle=9 late_lower=2`. The five
windows with a wait above 50 ms (57–223 ms) are the five with `vmo_runs` among the top holders;
the other five stay below 26 ms.

**Red in this cycle:** `SELFTEST: heap 32m FAIL` — the proof read its cost from the pool's free
count while an app exited and returned its frames; it reads the harness's own resident bytes
now. The USB control failure of the cycle before did not recur (the hub's billboard function
enumerated): judged a bus fault.


## Five boots, four frozen (`build/logs/board--2026-11-11T11-22-31`, build `dev-ba8b0f2c`)

The first build without the network services on the board. The operator booted five times:
four ended with a dead pointer or the test picture, one was complete (106 rungs, the nine known
board reds).

| boot | the trace ends at | last proof running |
|---|---|---|
| 1 | line 1381 | the harness's thread / workpool / supervised-restart proofs |
| 2 | line 1361 | the same (`workpool determinism FAIL` after its 5 s deadline) |
| 3 | line 1364 | the same |
| 4 | line 963 | right after `init: up gpud` |
| 5 | complete | — |

No trace carries a kernel word: the block service appends the console ring every 250 ms, a
dead kernel never reaches the disk (TASK-0330 P7). All four end after the fault-probe
fixture's kills (`[USER-PF]`).

**Hypotheses, in the order they fell:** the USB bring-up's timing (refuted: the pointer's
interface enumerated in boots 1 and 3); a hang inside the workpool proof (refuted: boot 4
stopped long before it); a wait's deadline nothing wakes (real — a waitset's and a fence's
deadline is woken only by another task's syscall, TASK-0363 P6 — but the block service's
250 ms timer keeps those sweeps going, and it stopped too).

**Reproduced on QEMU:** four harts and no network device (the launcher with an empty netdev,
so init withholds the same three services): 1 kernel page fault in 4 boots, after four
fault-probe kills.

**Cause:** a hart left without a task by a fault kill or by an order stayed on the dead
task's page table; the parent's reap on another hart freed it; the idle hart's next
translation miss walked freed frames. The network owner's `yield` loop had given every idle
hart work within a millisecond — deleting it opened the window, it did not create the fault.
The rule and its proof: `docs/kernel/hart-spaces.md` (TASK-0330 P6).

## Six boots with the rule in place (`build/logs/board--2026-11-11T13-43-55`, build `dev-0cdd0c91`)

Six boots in the trace (the one after the flash and five by the operator, the last with the
desktop used): every one prints `KSELFTEST: address space held by hart ok`, withholds the three
network services, passes the thread and workpool proofs and reaches
`SELFTEST: input window open`; no `PANIC`, no `KSELFTEST: address space free FAIL`. The last
boot's ladder is complete (106 rungs, the nine known board reds). Were 1 of 5.

On QEMU, four harts without a network device (`build/logs/smp4-nonet--2026-11-11-before`,
`…-after`): before the rule 1 kernel page fault in 4 boots; with it the boots pass the thread
and workpool proofs with no fault. The first cut of the rule left the space also when a process
reaped one of its own threads — ten of ten boots hung before `SELFTEST: thread spawn ok`; the
boot proof now holds that case (`kept`).
