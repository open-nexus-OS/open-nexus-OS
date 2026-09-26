<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# eMMC + SDHCI measurement — 2026-09-24 (TASK-0246 P0)

What the board's system disk and its host controller really do, what the upstream binding
says about them, and what QEMU can offer as a deterministic stand-in before the board runs our
OS. Taken from the stock system over `adb` (root, kernel 6.6.63, the board on the desk), from
the mainline tree and driver (facts only — register offsets, bits, bus layout; no code is
copied) and from QEMU 11.1.1 on the host.

Files: `ext_csd.hex` (the card's 512-byte EXT_CSD as the stock kernel read it — card data,
not vendor source), `qemu-sdhci-pci-qtree.txt` (QEMU monitor excerpt), `live-tree-sdh.txt`
(the three hosts' live tree properties, added 2026-09-26 — see the addendum at the end).

## The card (`mmc2:0001`, `/sys/bus/mmc/devices` + debugfs)

| Field | Value |
|---|---|
| Name / manufacturer | `AJTD4R`, manfid `0x15`, oemid `0x0100`, date 02/2024, fw `0x06…` |
| EXT_CSD_REV | 8 (eMMC 5.1), CSD structure 2 |
| Capacity | `SEC_COUNT` = 30 535 680 × 512 B = 14.56 GiB (sector addressing) |
| Speed modes (`CARD_TYPE` 0x57) | HS26, HS52, DDR52 1.8 V, HS200 1.8 V, HS400 1.8 V; `STROBE_SUPPORT` = 1 |
| Hardware partitions | `boot0`, `boot1` 4 MiB each (`BOOT_SIZE_MULT` 32), RPMB 4 MiB (`RPMB_SIZE_MULT` 32); `PARTITION_CONFIG` 0 (no boot partition enabled — the boot ROM reads the user area); `PARTITION_SUPPORT` 7, setting not completed |
| Erase | erase group 512 KiB (`HC_ERASE_GRP_SIZE` 1, `ERASE_GROUP_DEF` 1), preferred erase 4 MiB |
| Cache / queue | 64 MiB cache, enabled (`CACHE_CTRL` 1); command queue supported, depth 16, **off** in the stock kernel (`cmdq_en` 0) |
| Timeouts | `GENERIC_CMD6_TIME` 100 ms, `PARTITION_SWITCH_TIME` 20 ms, `OUT_OF_INTERRUPT_TIME` 100 ms, `S_A_TIMEOUT` 17 |
| Health | life time A/B 0x01, pre-EOL 0x01 (new) |
| Contents | first 256 bytes zero (never written, 2026-09-22) |

## The host (`sdh@d4281000`, the eMMC's)

- **Operating point (`/sys/kernel/debug/mmc2/ios`):** HS400 **enhanced strobe**, 8 bit, 1.8 V
  signalling, driver type B, requested 200 MHz, actual **187.5 MHz** = the node's
  `spacemit,sdh-freq` 375 MHz divided by 2 (standard SDHCI 10-bit divided clock). No tuning is
  involved in HS400ES (the strobe latches data).
- **DMA:** `mmc2: SDHCI controller on d4281000.sdh … using ADMA` — **32-bit ADMA2** (the stock
  driver prints `ADMA 64-bit` when it uses the 64-bit form). All three hosts say the same.
- **Errors since boot:** 0 in every class (`err_stats`).
- **Live node (vendor tree):** `compatible = "spacemit,k1-x-sdhci"`, `bus-width = 8`,
  `non-removable`, `no-sd`, `no-sdio`, `mmc-hs400-1_8v`, `mmc-hs400-enhanced-strobe`,
  `spacemit,sdh-freq = 375000000`, clocks `sdh-io` + `sdh-core`, resets `sdh_axi` + `sdh2`,
  `power-domains = <… 0>`, `interrupts = <101>`, `interconnects`/`interconnect-names = "dma-mem"`.
  The microSD host `sdh@d4280000`: 4 bit, `no-mmc`, `no-sdio`, card-detect GPIO 80 inverted,
  204.8 MHz, pinctrl `default` + `fast`, an `aib-clk`, and vendor voltage-switch registers.
- Linux host caps (`caps` 0x43df014f): 4/8-bit, MMC/SD high speed, non-removable, UHS
  SDR12…SDR104/DDR50, CMD23, driver types A/C/D, `NEED_RSP_BUSY`.

## Upstream facts (mainline `k1.dtsi`, `sdhci-of-k1.c`, binding `spacemit,k1-sdhci`)

- **Compatible:** `spacemit,k1-sdhci` (the vendor tree says `spacemit,k1-x-sdhci`); clocks
  `core` (= `sdh_axi`) + `io` (= `sdhN`), resets `axi` (shared) + `sdh` (per host), IRQ 101.
- **The hosts sit on `storage-bus`**, a `simple-bus` with
  `dma-ranges = <0x0 0x0 0x0 0x0 0x0 0x80000000>`: a storage master reaches **only the first
  2 GiB of physical memory**, identity-mapped (bus address = CPU address). The board's second
  bank (4 GiB at `0x1_0000_0000`) is unreachable for it. `usb@c0a00000` (DWC3) sits on the same
  bus; the two GMACs on `network-bus`, the PCIe hosts on `pcie-bus`, and `camera-bus`,
  `dma-bus`, `multimedia-bus`, `network-bus`, `pcie-bus` carry a SECOND range that TRANSLATES:
  e.g. bus `0x8000_0000…` → CPU `0x1_0000_0000…` — for those masters the address a device is
  programmed with is not the CPU physical address. The display controller and the GPU are not
  in the mainline tree yet (their bus is B1.7's measurement). The whole `soc` bus carries
  `dma-noncoherent`.
- **Controller quirks (K1):** data timeout counted in SD clocks, no END attribute in NOP
  descriptors, 32-bit ADMA length field, capability clock base unusable (the base clock is the
  `io` clock's rate), no card-detect, broken timeout value, **64-bit DMA broken**, preset
  values broken; the core always waits for busy on R1b (`NEED_RSP_BUSY`).
- **Vendor registers (beyond the standard 0x00–0xFF map):** `0x108` OP_EXT (bit 11 override
  clock output enable, bit 12 force clock on — SD-only hosts), `0x10C` LEGACY_CTRL (bit 6
  generate pad clock), `0x114` MMC_CTRL (bit 1 misc IRQ enable, bit 2 misc IRQ, bit 8 enhanced
  strobe enable, bit 9 HS400, bit 10 HS200, bit 12 MMC card mode), `0x118` RX_CFG (SD-clock
  select), `0x11C` TX_CFG (bit 30 internal-clock TX select for timings ≤ SDR50, bit 31 TX mux),
  `0x130`/`0x134` delay-line control/config (tuning only), `0x160` PHY_CTRL (bit 0 PHY
  function enable, bit 1 PLL lock, bit 31 legacy mode), `0x164` PHY_FUNC (bit 15 HS200 read
  FIFO), `0x168`/`0x16C` PHY DLL config (pre-delay 1, full range 1, vreg 1; reg1 0x92; bit 31
  DLL enable), `0x170` DLL status (bit 0 locked, ≤ 100 µs), `0x178` PHY pad config (drive
  select 3 bits = 4, bit 5 RX bias).
- **Sequence the upstream driver uses:** after a full reset: PHY function enable + PLL lock,
  pad drive 4 + RX bias, MMC card mode (eMMC hosts), pad clock on. Timing ≤ SDR50 sets the TX
  internal-clock select, faster clears it; HS200/HS400 set their MMC_CTRL bit next to the
  standard UHS select; 1.8 V in HOST_CONTROL2. HS400 enhanced strobe: set the strobe enable,
  then initialise the PHY DLL and wait for its lock. HS200 and SDR104 need delay-line tuning;
  HS400ES does not.

## QEMU 11.1.1 as the stand-in (`qemu-sdhci-pci-qtree.txt`)

- No sysbus SDHCI is user-creatable on `virt`; the only SD host is **`sdhci-pci`** (PCI
  `1b36:0007`, class `0805`, `00:01.0` when it is the first device, BAR0 memory 256 bytes,
  **unassigned** — nothing on this machine assigns BARs before the OS).
- Default capabilities `capareg = 0x057834b4`: SDHCI spec v2, base clock 52 MHz, ADMA2 +
  SDMA, high speed, 3.3 V + 1.8 V, 512-byte max block, **no 8-bit bus**, no 64-bit bus —
  `capareg` and `sd-spec-version` are device properties, so the lane can declare an 8-bit v3
  host.
- **`emmc`** is a real eMMC model on the host's SD bus (`boot-partition-size`, `boot-config`,
  `rpmb-partition-size`), so the eMMC init path (CMD1, CMD2, CMD3, CMD7, CMD8 ext-CSD, CMD6
  switches) runs against it; the vendor registers and HS200/HS400 do not exist there.
- **PCI on `virt`:** `pci-host-ecam-generic` at `0x3000_0000` (ECAM, buses 0–255), 32-bit
  memory window `0x4000_0000`–`0x7fff_ffff`, 64-bit window at `0x4_0000_0000`, INTx through
  `interrupt-map` (device 1, pin A → PLIC 33). The OS has no PCI code today.

## What this changes for TASK-0246 (read with the ledger)

1. **DMA reach is a device property.** A storage master can address only `[0, 2 GiB)`; today
   a DMA buffer lands there only because the frame pool fills bank 0 first. The tree must say
   it (`storage-bus` with its `dma-ranges` in `config/board/bpi-f3/board.dts`), the device
   capability must carry it (like the interrupt line and the coherence), and the kernel must
   allocate a device's DMA memory within its reach and answer `vmo_runs` in its BUS addresses.
2. **No tuning on the way to full speed:** HS52 (8 bit, SDR) first, then HS400 enhanced strobe
   (DLL lock, no tuning) — the stock system's own operating point. HS200 is skipped.
3. **32-bit ADMA2** is the descriptor format (it is also the only one the reach allows).
4. **QEMU proves the standard core** (reset, clock, commands, ADMA2, IRQs, the eMMC init to
   HS52 8-bit) through `sdhci-pci` + `emmc`, which needs PCI ECAM enumeration and BAR
   assignment in the OS — deterministic, and the same planner serves nxboot and init. The
   board proves the K1 layer (vendor registers, HS400ES).

## Addendum 2026-09-26 (TASK-0246B P2): which host may hold what (`live-tree-sdh.txt`)

Read over adb from the live vendor tree (kernel 6.6.63), the standard mmc-controller flags that
say what each host may carry:

| Host | Carries | Flags in the live tree | `bus-width` | `spacemit,sdh-freq` |
|---|---|---|---|---|
| `sdh@d4280000` | the microSD slot | `no-mmc`, `no-sdio` (+ `cd-inverted`) | 4 | 204.8 MHz |
| `sdh@d4280800` | the SDIO WiFi function | `no-mmc`, `no-sd`, `non-removable` | 4 | 375 MHz |
| `sdh@d4281000` | the eMMC | `no-sd`, `no-sdio`, `non-removable`, `mmc-hs400-1_8v`, `mmc-hs400-enhanced-strobe` | 8 | 375 MHz |

The frequencies agree with the APMU state measured on 2026-09-22
(`../2026-09-22-stock-system/regmap-apmu.txt`): `SDH0` = `0x411b` selects mux 0 (`pll1_d6`,
409.6 MHz) divided by 2 = 204.8 MHz; `SDH1` and `SDH2` = `0x52` select mux 2 (`pll2_d8`,
375 MHz) divided by 1.

