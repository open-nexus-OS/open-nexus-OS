<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# DRAM retention across the board's reset — 2026-09-28 (TASK-0327B P3)

Does the DRAM keep its content across a reset of the reference board? RFC-0107 Phase 3's RAM
rescue depends on it, and the first eMMC boots with the OS trace (TASK-0327B P2) kept no OS
text, so the only reader of the kernel's console ring left was the next loader.

Method (`trace-all.txt`, four eMMC boots read with `just board-log`, no serial adapter):

1. Before it loads its image over the kernel window, nxboot writes a probe page — a magic and
   the boot's trace sequence number — into the window's last page and, on the next boot, reads
   it first (`nxboot: dram probe kept|stale|lost`).
2. It then scans the window's page starts for the kernel's console ring header
   (`nxboot: rescue ok|none (…)`), stamped with the previous boot's number or unstamped.
3. Reset by the board's reset button, the microSD out (the boot ROM boots the eMMC), twice; the
   stock system read the trace afterwards.

Result:

- Boot 1: `dram probe none (first boot)`.
- Boots 2, 3, 4: `dram probe lost` and `rescue none (no ring in ram)`, every time.

Verdict: **the reset path does not keep DRAM content** — the vendor SPL trains the DDR on every
boot and the page nxboot wrote is gone, so a ring the kernel wrote would be gone too. Phase 3
cannot rescue a boot on this board; it stays for QEMU's reset lane (where the ring survives and
the rescue is proven) and for boards that keep their DRAM.

What the board's boots still say: every boot's loader ran to `nxboot: jump slot=a base=0x400000`
(the kernel image verified), and no boot kept OS text — the kernel did not reach the block
owner's trace writer. How far it comes before that needs a channel the reset does not erase:
the debug UART (an adapter), or the board's user LEDs driven by the kernel's early milestones.

## The board's user LED, measured on the stock system (for a kernel-side ladder without a UART)

- `/sys/class/leds`: `sys-led` (trigger `heartbeat` on the stock system; the three `mmcN::`
  LEDs are software triggers, not separate lights the user saw).
- `/sys/kernel/debug/gpio`: `gpio-96 (sys-led) out lo` on `gpiochip0` = `k1x-gpio`, 128 lines.
- The controller: `/soc/gpio@d4019000`, compatible `spacemit,k1x-gpio`, `reg = <0xd4019000 0x800>`.
- The tree's `leds/led1`: `label = "sys-led"`, `gpios = <&gpio 96 0>` (active high),
  `linux,default-trigger = "heartbeat"`.
- Only the power LED was lit during our eMMC boots: the stock kernel drives this LED; our chain
  leaves it off. A kernel that toggled it at its early milestones would be visible without any
  adapter — a few bits, where the UART gives the text. Whether the pad is muxed as GPIO before
  the stock kernel runs is not measured.

## Amendment, later the same day: the retention is marginal, not absent

A fourth cycle (the boot LED package, `trace-all-led-cycle.txt`) kept the probe across the
reset button — `nxboot: dram probe kept (seq=1)` — and the loader rescued the previous boot's
console ring: `nxboot: rescue ok (seq=1 bytes=6635 lost=0)`. The text
(`kernel-console-boot1-rescued.txt`) is the board's first kernel console: the platform from the
tree, the high half, four harts online, every kernel selftest, and the spawn of init up to its
segments being mapped — where it ends, mid-line, 117 counted bytes reading as zero. A few bytes
inside are flipped (`selft%st`, `JSELFTEST`): the DRAM was decaying while it was read.

So: the reset does not scrub the DRAM deterministically; the content decays over the reset
(likely with the time the SoC spends unpowered/retraining), and a quick reset can keep most of
it, with bit errors. The rescue is therefore worth having on this board — as a lucky witness,
never as a proof medium — and the LED ladder is the deterministic one.


## Files added 2026-09-28/29 (TASK-0260B P3)

- `kernel-console-led-window-boots1-3.txt` — the first kernel console rescued after the LED
  window was mapped: the whole kernel selftest ladder on the board, three boots, `lost=0`.
- `board-boot-2026-09-29-first-os-trace.txt` — the first boot whose OS trace reached the eMMC
  (`blkd: trace os ok`): init, the fleet, bundlemgrd serving the system volume; init died on the
  missing virtio-net (fixed the same day).
- `board-boot-2026-09-29-verified-4harts-led.txt` — the verification boot with the board's own
  tree, four harts and the LED ladder: `smp bringup ok mask=0xf`, `plic ctx cpu0..3 ok`,
  `init: ready`, `blkd: backend ok (spacemit,k1-sdhci mode=hs400es)`, the OS trace on disk.
