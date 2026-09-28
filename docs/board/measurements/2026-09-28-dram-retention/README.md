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
