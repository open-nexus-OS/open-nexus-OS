<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# 2026-11-11 — every hart the tree names runs (TASK-0330 P1, RFC-0118)

Status: **proven on QEMU (one, two and eight harts) and on the board (eight harts, two clusters); a device interrupt is taken on the hart its driver runs on (TASK-0330 P8, last section).**

## Question

Does the kernel run the harts the device tree names — per-hart state sized at boot, no constant
counting harts — and does it lose none of eight in ten boots? Before P1 it started four of the
eight the tree named and did not name the other four
(`../2026-11-10-smp-baseline/README.md`, "Eight harts in the tree").

## Instrument

| Line | Says |
|---|---|
| `KGATE: smp cpuN hart= online= asm= stage= hsm=` | per CPU: its hart, whether it reached the entry stub, how far its first Rust got, what the firmware says |
| `KGATE: smp bringup ok mask=` / `… DEGRADED expected= got=` | the verdict against every CPU of the machine |
| `KSELFTEST: per-hart state ok (harts= unsized=0)` | every per-hart table holds exactly the machine's CPUs, none was forgotten at sizing |
| `KSELFTEST: smp online ok harts= clusters=` | the CPUs online and the tree's clusters |
| `init: affinity applied n= fail= online=` | the placements init applied by role against the online set the kernel reported |

## One hart and two harts

- `just ci-os-smp1` green (`build/logs/smp1--2026-11-11T16-06-05`):
  `per-hart state ok (harts=1 unsized=0)`, `init: affinity applied n=0x22 fail=0x0 online=0x1`,
  `ipc call budget ok (rt=51us mean=53us budget=64)`, `console off lock ok (max_us=96 …)`.
- `just ci-os-smp` green on the first attempt (`build/logs/smp--2026-11-11T16-17-17`) — the
  lane's first green since at least 2026-10-20: `smp bringup ok mask=0x3`,
  `per-hart state ok (harts=2 unsized=0)`, `smp online ok harts=2 clusters=1`,
  `init: affinity applied n=0x22 fail=0x0 online=0x3`, the lock ping, IPI, steal and shootdown
  proofs. The run before it (`…T16-12-51`) was red on two timing verdicts alone
  (`ipc call budget FAIL (rt=572us …)`, `console off lock FAIL max_us=1137`): several harts run
  under multi-thread emulation, where the guest's clock is the host's wall clock. Those two are
  rungs on a multi-hart lane now (`config/fail-marker-allow-mttcg.txt`) and stay verdicts on the
  one-hart lanes and the board.

## Eight harts, ten boots

`NEXUS_PROFILE_ENV_OVERRIDE=1 SMP=8 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s just test-os smp`, ten
times in a row on an eight-thread host (`build/logs/smp--2026-11-11T16-20-04` … `T16-43-23`).

```text
nxboot: fdt ok (harts=8 …)
KINIT: cpu0 online … KINIT: cpu7 online
KGATE: smp cpu1 hart=1 online=1 asm=1 stage=4 hsm=0
…
KGATE: smp cpu7 hart=7 online=1 asm=1 stage=4 hsm=0
KGATE: smp bringup ok mask=0xff
KSELFTEST: per-hart state ok (harts=8 unsized=0)
KSELFTEST: smp online ok harts=8 clusters=1
init: affinity applied n=0x22 fail=0x0 online=0xff
```

| Result | Count |
|---|---|
| boots with all eight harts online (`mask=0xff`) | 10 of 10 — 0 lost harts |
| kernel panics or page faults | 0 |
| `per-hart state ok (harts=8 unsized=0)` | 10 of 10 |
| placements applied by init / refused | 34 / 0 in every boot |
| boots that reached `init: ready` and the ladder's end | 10 of 10 |
| a CPU entered by another hart than the tree names | 0 |

QEMU's `virt` tree has no `cpu-map`: one cluster. The two clusters of four are the board's.

## The lock's wait budget on a multi-hart lane: the host's page faults, not the lock

The first eight-hart series ran 3 green, 7 red — on one line — and the two-hart lane then went
red three attempts in a row on the same line:

```text
KSELFTEST: bkl budget FAIL max_wait=15302us … 155080us max_hold=2–4ms   (eight harts, 7 of 10)
KSELFTEST: bkl budget FAIL max_wait=65169us … 120160us max_hold=2ms     (two harts, 3 of 3)
```

One wait of 44–155 ms per boot while the longest syscall held the lock 2–4 ms. Three
hypotheses, each with its criterion, three instruments (they stay in the kernel, each prints
its first few lines only):

| Line | Says |
|---|---|
| `KINIT: bkl long wait <us> cpuN for=<id> deferred=<n> passed_over=<n>` | a wait above 10 ms: the hart, what it wanted the lock for (`lock_share` id), how many loop turns it stood back for CPU 0's right of way, how often the lock changed hands meanwhile |
| `KINIT: bkl long hold <us> cpuN by=<id>` | a hold above 10 ms by ANY holder — the ecall budget sees syscalls only, not a fault, the timer or the scheduling loop |
| `KINIT: fault long step=<fill\|pressure\|exit> us= pid= va=` | which step of a fault held the lock above 10 ms |

| Hypothesis | Criterion | Measured |
|---|---|---|
| a hart stands back for CPU 0's right of way until its bound of 200 000 turns runs out | long waits show `deferred=200000` on a CPU ≠ 0 | refuted: `deferred=0` in every long wait, and CPU 0 waits 40–79 ms too (`build/logs/smp--2026-11-11T17-10-49` … `T17-21-08`) |
| the lock is not fair: a waiter is passed over many times | `passed_over` in the hundreds | refuted: `passed_over=0` — ONE holder; `bkl long hold 52565us cpu0 by=83` (a fault) beside `bkl long wait 44141us cpu1 for=84` (`…T17-25-35` … `T17-49-16`) |
| the host's memory envelope throttles the lane | the lane cgroup's `high` events above 0 | refuted: `events: low 0 high 0 max 0`, peak 756 MB of 4 GB |

What it is: `fault long step=fill us=52096 … 119748 pid=63` — ONE page fill of the selftest
client. A probe inside the fill (not kept) split it:

```text
KINIT: commit probe alloc_us=7 zero_us=28413 enter_us=12 pa=0x92a00000
KINIT: commit probe alloc_us=6 zero_us=20194 enter_us=15 pa=0x93600000
```

Zeroing one 4 KiB frame took 20–28 ms, both times at a 2 MiB-aligned physical address: the
guest's first touch of a fresh 2 MiB region of its RAM is a page fault of the HOST, which hands
QEMU a transparent huge page there and compacts memory to find one (this host: huge pages
`always`, 600 MB free, 135 022 compaction stalls counted). The kernel lock is held across that
zeroing, so the other hart's wait inherits the stall. On one hart under icount the same boot
spends 54 ms in all 8 427 faults together — the fill is long in the host's time, not in
instructions. Real hardware has no such fault.

The fix is where the cause is: QEMU preallocates the machine's memory (`-mem-prealloc`,
`scripts/qemu-launcher.sh`; `QEMU_MEM_PREALLOC=0` turns it off). Two harts after it, six boots
(`build/logs/smp--2026-11-11T18-13-35` … `T18-27-23`):

| Line | Before (5 boots) | After (6 boots) |
|---|---|---|
| `KSELFTEST: bkl budget` | 2 ok, 3 FAIL (44–120 ms) | 6 ok (`max_wait` 2.3–10.1 ms) |
| `KINIT: fault long step` | in every red boot | none |
| `console off lock` | 1.0–1.3 ms, FAIL in 2 of 3 | 0.7–5.1 ms, FAIL in 4 of 6 — a rung on this lane |
| `ipc call budget` | `rt=` 500–572 µs, FAIL | `rt=` 563–662 µs, FAIL — a rung on this lane |

The lock's wait budget therefore stays a verdict on the multi-hart lane (it is not on
`config/fail-marker-allow-mttcg.txt`).

Eight harts after it, ten boots on the final tree (`build/logs/smp--2026-11-11T18-34-13` …
`T19-02-21`; the one-hart lane before them green, `smp1--2026-11-11T18-30-48`):

| Result | Before (10 boots) | After (10 boots) |
|---|---|---|
| all eight harts online, `per-hart state ok (harts=8 unsized=0)`, `init: ready` | 10 | 10 — 0 lost harts, 0 panics |
| placements applied / refused | 34 / 0 | 34 / 0 |
| `KINIT: fault long step` | — (not built yet) | none in any boot |
| `KSELFTEST: bkl budget ok` | 3 (`max_wait` up to 155 ms in the reds) | 8 (`max_wait` 5.8–12.6 ms) |
| `KSELFTEST: bkl budget FAIL` | 7 | 2 |

The two reds are the kernel lock itself at eight harts on eight host threads, and they stay
red (the eight-hart lane is not a CI lane yet; it joins `test-all` with TASK-0330 P4):

- `…T18-40-26`: `max_hold=24ms nr=11` — the exit syscall held the lock 24 ms inside the judged
  window (`KINIT: long ecall nr=11 24ms cpu3`, later `57ms`). Exit holds 11–15 ms once in every
  eight-hart boot and never above 10 ms at two harts. Not yet measured: which step of exit.
  Criterion on the board: a `bkl long hold … by=11` line at eight real harts is the kernel's
  cost (then the step is measured like the fault's); none means eight emulated harts on eight
  host threads.
- `…T18-46-50`: `max_wait=20292us max_hold=5ms`, but ten waits above 10 ms against a budget of
  four (`gt10ms=10`; 17 in the other red boot, 0–3 in the eight green ones). No holder is long
  there — the waiters were passed over 4–17 times (`passed_over=`): seven harts queue behind
  holds of 1–5 ms. The convoy budget was calibrated at two and four harts.

What the new lines still show, for the phases that take the lock apart (TASK-0330 P2–P4):

- `bkl long hold 21–36 ms cpu0 by=48` once in every boot, the other hart waiting as long
  (`for=26`, `for=84`): the telemetry operation of the sched syscall formats its report under
  the lock, before the budget's window opens.
- `bkl long hold 11–127 ms by=69` and `10–13 ms by=63` during bring-up (outside the judged
  window, like `KINIT: bkl bring-up burst`).
- At eight harts the telemetry operation holds 61–90 ms (`long ecall nr=48`) once per boot.

## The board: eight harts, two clusters

Build `dev-c22532d5`, `build/logs/board--2026-11-12T10-55-43` (the operator's session: desktop,
apps, tiling, typing; before P1 the same board ran four of its eight harts).

```text
nxboot: fdt ok (harts=8 tb=24000000Hz …)
KGATE: smp cpu1 hart=1 online=1 asm=1 stage=4 hsm=0   … cpu7 hart=7 the same
KGATE: smp bringup ok mask=0xff
KSELFTEST: per-hart state ok (harts=8 unsized=0)
KSELFTEST: smp online ok harts=8 clusters=2
KSELFTEST: plic ctx cpu0 ok … cpu7 ok, plic isolation ok
init: affinity applied n=0x1f fail=0x0 online=0xff
KINIT: bkl bring-up burst max_wait=4749us max_hold=2ms nr=6 gt10ms=0
KSELFTEST: ipc call budget ok (rt=32us mean=38us budget=64)
KSELFTEST: console off lock ok (max_us=509 budget_us=1000)
KSELFTEST: bkl budget ok (max_wait=2476us max_hold=1ms nr=20)
```

| Question | Answer on the board |
|---|---|
| harts online / clusters | 8 of 8 / 2 (was 4 of 8) |
| placements applied by init / refused | 31 / 0 against `online=0xff` |
| `KINIT: bkl long wait`, `bkl long hold`, `fault long step`, `long ecall` | none — not one in the whole session |
| the exit syscall's 24 ms hold of the eight-hart QEMU lane | not the kernel's cost: no `bkl long hold … by=11`; the longest syscall hold of the judged window is 1.6 ms (`nr=20`) |
| waits for the lock in the judged window | `le100us=929208 le1ms=446 le10ms=89 gt10ms=0` |
| the lock's holders with the desktop in use (eleven 10 s windows) | 49–58 % of a window, of it 38–45 points the idle loop's takes (`id=84`); everything else 4.5–20 % |
| interactive ready latency over the session | n=284 609, 98.8 % within 100 µs, 36 above 10 ms (longest 167 ms) |
| wake IPI | mean 2 µs, longest 38 µs |

The board ladder is complete (106 rungs, `scripts/board-test.sh --profile=board-visible`; the
nine known board reds tolerated). The operator markers were not recorded for this capture.

## A device interrupt on the hart its driver runs on (TASK-0330 P8, 2026-11-12)

Before: every bound line was enabled in CPU 0's context and only CPU 0 unmasked the external
interrupt (read from the code at `ee10ce1a`: `irq::bind` targeted the boot CPU, `sie.SEXT` was
set in `kmain` only, the idle delivery was the boot hart's) — a driver homed on another hart
was woken across harts. Instrument: `KSELFTEST: irq off boot hart ok (cpu= irq=)`, printed by
the first CPU other than the boot hart that claims a bound line and delivers it.

| Machine | Line | Log |
|---|---|---|
| QEMU, one hart | absent (as it must be), lane green | `build/logs/smp1--2026-11-12T11-53-14` |
| QEMU, two harts | `irq off boot hart ok (cpu=1 irq=5)` — the block device; lane green on the first attempt | `build/logs/smp--2026-11-12T11-56-27` |
| QEMU, eight harts, three boots | `cpu=2`, `cpu=7`, `cpu=6`, each `irq=5`; all three green, `bkl budget ok` (`max_wait` 5.9–6.7 ms) | `build/logs/smp--2026-11-12T11-58-47` … `T12-03-59` |
| the board, eight harts | `irq off boot hart ok (cpu=6 irq=101)` — the storage host | `build/logs/board--2026-11-12T12-31-01` (build `dev-0f0ef702`) |

The board in that boot: ladder complete (109 rungs with the three this phase adds to
`scripts/board-test.sh`; the four-hart log of 2026-11-11 fails on the first of them), storage up
on the host controller (`blkd: backend ok (kind=…sdhci … mode=hs400es)`), keyboard and pointer
live, `KSELFTEST: bkl budget ok (max_wait=416us max_hold=0ms)` (the cycle before: 2 476 µs),
`ipc call budget ok (rt=45us)`, waits for the lock in the judged window
`le100us=1106046 le1ms=180 le10ms=0 gt10ms=0`, no `bkl long` line. One boot each side — the
shorter wait is an observation, not a claim about the cause. The operator markers were not
recorded for this capture.
