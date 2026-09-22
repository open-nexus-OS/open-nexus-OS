<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Measurement 2026-09-22 — the stock system on the reference board, read over adb

Block 1 P0 of the hardware fast track (`tasks/IMPLEMENTATION-ORDER.md`, R1–R5/R14). Every
number here was read from the running stock system (the raw boot log, the U-Boot environment
and the package inventory are not stored — they carry vendor names; their facts are in this file) (kernel 6.6.63, root shell over the adb
gadget `361c:0008`) — nothing was written to the board. The live device tree was dumped and
decompiled for reading but is NOT stored here: it is compiled from the vendor kernel's
GPL-licensed source; the facts below are ours, the mainline `k1.dtsi` (GPL-2.0 OR MIT) is
the source our `config/board/bpi-f3/board.dts` derives from.

| File | What |
|---|---|
| `cpuinfo.txt` | the 8 harts: ISA string, uarch, vendor/arch/impl ids |
| `iomem.txt` | the physical map as the stock kernel sees it (RAM banks, reserved ranges, every MMIO block it claimed) |
| `interrupts.txt` | PLIC source numbers per device (the IRQ truth; the irqchip's vendor label scrubbed) |
| `clk_summary.txt` | debugfs clock tree head: which clocks are on under `clk_ignore_unused` |
| `partitions.txt`, `lsblk.txt` | block devices: SD (stock), eMMC (empty), boot partitions |
| `lsmod.txt` | loaded modules (the vendor WiFi + GPU drivers are built in) |
| `edid-hdmi.bin` | the desk monitor's EDID (256 B) |

## Facts (curated)

**SoC / CPU.** `spacemit,k1-x`, model "spacemit k1-x deb1 board" (= the FIT DTB `k1-x_deb1`);
8 × `spacemit,x60`, `cpu-map` = two clusters of four; ISA
`rv64imafdcv_zicbom_zicboz_zicntr_zicond_zicsr_zifencei_zihintpause_zihpm_zfh_zfhmin_zca_zcd_zba_zbb_zbc_zbs_zkt_zve…_sscofpmf_sstc_svinval_svnapot_svpbmt`;
`mmu-type riscv,sv39`; L1 32 KiB I + 32 KiB D (64 B lines), L2 512 KiB per cluster;
`timebase-frequency` **24 000 000 Hz** (QEMU virt: 10 MHz — `TICKS_PER_US = 10` dies);
`mvendorid 0x710`, `marchid 0x8000000058000001`, `mimpid 0x1000000049772200`.

**SBI.** OpenSBI spec v1.0, impl ID 1 version 0x10003; IPI, RFENCE, HSM, SUSP, PMU
extensions; `earlycon=sbi`. Sstc present ⇒ `stimecmp`; `riscv,clint0` at `0xe400_0000`
exists but S-mode never needs it.

**Memory (R14).** Two banks: `memory@0` = 0 … 2 GiB and `memory@100000000` = 4 … 6 GiB
(4 GiB board; MemTotal 3 898 372 kB). Reserved by the stock system: `0x0–0x7ffff` (OpenSBI),
`0x100000–0x5fffff` (rcpu heap/vrings/rsc table), `dpu_reserved@2ff40000` 768 KiB nomap,
`framebuffer@7f000000` 16 MiB (the bootloader splash). U-Boot loads at `0x0020_0000`,
the kernel at `0x0800_0000`, dtb at `0x3100_0000`, initrd around `0x7cfd_5000`.

**Interrupts.** PLIC `riscv,plic0` at `0xe000_0000` (64 MiB), `riscv,ndev 159`, 16
contexts (8 harts × M/S), `interrupts-extended` M=11/S=9 per hart. Sources in use:
UART1 42, UART3 44, i2c 19/36/38, mmc0 (SD) **99**, mmc1 (SDIO WiFi) **100**, mmc2 (eMMC)
**101**, GPU `pvrsrvkm` **75**, xHCI **125**, EHCI 118, UDC 105, dwc3 wakeup 149, DPU
ONLINE **139** / OFFLINE **138**, HDMI 136, end0 **131**, end1 **133**, pcie 146/147,
thermal 61, mailbox 52, pdma 72, gpio 58, pinctrl 60, crypto 113, spi 117. The stock
kernel serves every PLIC source on hart 0.

**Console.** `serial0 = serial@d4017000`, compatible `spacemit,pxa-uart` (mainline:
`spacemit,k1-uart`, `intel,xscale-uart` — a 16550-class UART with 4-byte register stride),
`stdout-path = serial0:115200n8`, clock `slow_uart`. Nine more UARTs at `0xd4017100…800`.

**Clocks / resets / power.** ONE clock controller `spacemit,k1x-clock` and ONE reset
controller `spacemit,k1x-reset`, both at `0xd405_0000` with the register windows
`mpmu 0xd4050000/0x209c`, `apmu 0xd4282800/0x400`, `apbc 0xd4015000/0x1000`,
`apbs 0xd4090000/0x1000`, `ciu 0xd4282c00/0x400`, `dciu 0xd8440000/0x98`,
`ddrc 0xc0000000/0x4280`, `apbc2 0xf0610000/0x20`, `rcpu 0xc0880000`, `rcpu2 0xc0888000`,
`audpmu 0xc088c000` (mainline names them `spacemit,k1-syscon-apbc/mpmu/apmu`, PLL at
`0xd4090000`); inputs `vctcxo_24` (24 MHz), `vctcxo_3`, `vctcxo_1`, `pll1_2457p6_vco`,
`clk_32k`. Power domains: `spacemit,power-controller` (9 domains; BUS 0, VPU 1, GPU 2, …,
HDMI 7). R3: the stock kernel boots with `clk_ignore_unused` — it never gates what the SPL
turned on; `clk_summary.txt` shows uart/emac/pcie/dma/usb clocks enabled.

**DMA coherence (R4).** The CPU has `zicbom`/`zicboz` and `svpbmt`; the stock kernel runs
with `swiotlb=65536` and every DMA master carries `interconnects = <… "dma-mem">` ⇒ the
masters are **not** cache-coherent: our `DmaBuffer` needs cache maintenance (Zicbom) or
non-cacheable mappings (Svpbmt) before the first DMA driver (SDHCI ADMA).

**Storage.** Three `spacemit,k1-x-sdhci` hosts, 0x200 registers each: `sdh@d4280000`
mmc0 = microSD (stock, 29.8 GiB, SDR12, card-detect GPIO), `sdh@d4280800` mmc1 = SDIO
WiFi (SDR104), `sdh@d4281000` mmc2 = **eMMC** (14.6 GiB, HS400 ES, `AJTD4R` manfid 0x15,
`boot0`/`boot1` 4 MiB hardware partitions, RPMB). All three run **ADMA**. Clocks `sdh-io`,
`sdh-core` (+ `aib-clk` on SD), resets `sdh_axi` + `sdhN`, power domain 0. The eMMC's
first 256 bytes are all zero — it has never been flashed; the SD card's sector 0 is the
boot-ROM `bootinfo` header (`SDC`).

**Display (R5).** `display-subsystem-hdmi` = `spacemit,saturn-hdmi` at `0xc044_0000`
(0x2a000), pipeline `port@c0440000` = `spacemit,dpu-online2` (IRQs 139/138), encoder
`hdmi@c0400500` = `spacemit,hdmi` (0x200, IRQ 136, clock `hmclk`, reset `hdmi_reset`, power
domain 7). Also `display-subsystem-dsi` at `0xc034_0000` (`port@c0340000`, `dsi2@d421a800`,
`dphy2`). Desk monitor EDID: "Artist 24 Pro" (mfg UGD, 2019), preferred 2560×1440@59.95
(241.5 MHz) — the vendor driver picks **1920×1080@60** ("setting … as preferred mode"; the
SoC's HDMI ceiling), 8 bpc, `fb0`. `dpu_reserved` 768 KiB nomap.

**USB (R8).** DWC3 `snps,dwc3` at `0xc0a0_0000` (0x10000, IRQ 125, `dr_mode host`, UTMI HS
phy, glue `d4282bc8.usb3`, clock `usb30_clk`) → xHCI with two roots (HS 480 Mbps, SS
5 Gbps); EHCI `spacemit,mv-ehci` at `0xc098_0100` (IRQ 118); the OTG port is `udc@c0900100`
`spacemit,mv-udc` (IRQ 105) — the gadget we talk to. PHYs at `0xc0940000`, `0xc09c0000`
(`usbphy1`), `0xc0a30000` (`usb2phy`), `0xc0b10000` (`puphy`, USB3 combo).

**GPU (R6).** `imggpu@cac00000`, compatible **`img,rgx`**, regs `0xcac0_0000/0x80000`, IRQ
75, clock `gpu_clk`, reset, power domain 2, `dma-mem` interconnect. Hardware reports
**BVNC 36.29.52.182**, 1 core (the "B-series" core of this SoC is a Rogue-architecture
part — the mainline kernel driver for this GPU architecture and the open Vulkan driver target it;
this BVNC is listed there as unsupported/not under development). Firmware present:
`rgx.fw.36.29.52.182` (139 264 B, loaded) + `rgx.sh.36.29.52.182` (274 840 B), plus a second
pair for BVNC 36.56.104.183 (unused). Vendor stack: package `img-gpu-powervr 24.2`
(closed userspace window-system library), kernel driver `pvrsrvkm` DDK 24.2 built in.
Firmware license: none found in `/lib/firmware`; provenance for our gate is G0's job.

**Network (R7).** `ethernet@cac80000` / `cac81000` (`end0`/`end1`, IRQ 131/133, MACs from
the dts). WiFi = SDIO function `024c:b852` on mmc1, driver `rtl8852bs` **built into the
vendor kernel** (Dual MIT/GPL modinfo), `wlan0` up; firmware: `rtw89/rtw8852b_fw.bin`
(1 035 232 B) and `rtw8852b_fw-1.bin` present (the upstream rtw89 firmware family — the
same chip family's PCIe/USB firmware), BT firmware `rtlbt/rtl8852bs_fw`. No SDIO HCI in
mainline rtw89 as of the write-ups; the vendor driver corpus size could not be measured
(built in).

**Boot flow (R1).** Vendor U-Boot env: `bootcmd=run autoboot` → `mmc_boot` → `boot_kernel`
= `detect_dtb` (`product_name` → `dtb_name`, `k1_deb1 → k1-x_deb1.dtb`), `loadknl`
(`vmlinuz-6.6.63` from bootfs to `0x0800_0000`), `loaddtb` (from bootfs to `0x3100_0000`,
"use built-in dtb" on failure), `loadramdisk`, `bootm/booti`. So the DTB that reaches the
kernel is the one U-Boot loads from bootfs (`/boot/spacemit/6.6.63/k1-x_deb1.dtb`), with the
FIT's own `fdt_1 = k1-x_deb1` as the built-in fallback; `/chosen` carries
`u-boot,version`, `boot-hartid 0`, `stdout-path`. For nxboot as the FIT payload the question
"which DTB lands in `a1` when OpenSBI jumps to the FIT's `uboot` image" is answered by the
FIT config the SPL selects (`fdt_1` for this board) — measured at B1.6 on the serial console.
`mtdparts` names the SPI-NOR layout (`bootinfo 64K@0, private 64K@64K, fsbl 256K@128K,
env 64K@384K, opensbi 192K@448K, uboot -@640K`); the eMMC/SD layout is
`partition_universal.json` (`resources/board/bpi-f3/PROVENANCE.md`).

**Other.** PMIC `spacemit,spm8821` on i2c (regulators, RTC `pmic,rtc,spm8821`, power key);
SoC `rtc@d4010000`; `timer@d4014000/d4016000`; `watchdog@d4080000`; `crng@f0703800`;
`fuse@f0702800`; three PCIe controllers (`pcie@ca000000/ca400000/ca800000`); 153 SoC nodes
in the vendor tree, of which Block 1 needs about fifteen.
