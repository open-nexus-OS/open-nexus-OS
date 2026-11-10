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

## Board

Open: one board cycle, the lines above copied here, a verdict per hypothesis, and the split
order written as an ADR from H1–H3.
