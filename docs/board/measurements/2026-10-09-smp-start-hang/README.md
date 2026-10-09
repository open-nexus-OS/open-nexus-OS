<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# 2026-10-09 — a board boot hangs as the harts enter the scheduler (warm resets)

**What happened.** After the TASK-0068 closure image `dev-e5b3a848` was flashed, two boots in a
row showed a black screen. A cold power cycle (power removed for a few seconds) then booted
to the desktop. The kernel source is unchanged since the image `dev-f94c6744`, which booted the
same day; between the two images only userspace bundles differ (the shell's capture panel,
app-host's modal sync), and neither runs before init's first line.

**How it was captured.** No UART adapter is on the desk; the evidence is the eMMC trace the
loader keeps (`just board-log`): the second boot's loader lines (`trace slot=1 seq=2`) and the
first boot's console ring, rescued from DRAM by the loader (`rescue ok (seq=1 bytes=8504
lost=0)`) — `trace-all-dev-e5b3.log`. The single flipped bits in the rescued text (`[INFO
bokt]`, `SYSBALL instald`) are DRAM retention across the reset, not the fault.

**Where it stops.** The kernel runs every boot selftest, spawns init (`spawn ok pid=17`, ~1.8 s),
three harts print `KINIT: cpuN sched loop` (1, 3, 0 — cpu2 not yet) and the ring ends mid-line
(`[INFO smp`), before init's first line. On the good boot the same point continues with
`KINIT: cpu2 sched loop`, `KINIT: cpu1 pick pid=16 …` and the BKL waits. The second boot left
no OS trace at all (blkd never wrote one), so it stopped before userspace too.

**Reading.** An intermittent hang at the start of SMP scheduling, seen after warm resets only
(two of two warm boots of this image, one of one cold boot fine; the earlier images of the day
booted after warm resets). Not reproduced on QEMU. Owner: the SMP lane — TASK-0330 S0/S1
(secondary-hart start and the per-hart structures; the HSM retry bug class), measured there
before any kernel edit.
