<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Reference board: BPI-F3 (SpaceMiT K1)

The board on the desk for the hardware fast track (`tasks/IMPLEMENTATION-ORDER.md`). This page
is what a developer needs to reach it; the boot chain we build on it is Block 1's subject
(RFC-0098, ADR-0066/0067, `TASK-0260B`). Measured facts carry the date they were measured; the
full read-out of the stock system (2026-09-22) is
`measurements/2026-09-22-stock-system/README.md`. The eMMC, its SDHCI host, the storage bus's
DMA reach and QEMU's SD stand-in (2026-09-24, TASK-0246 P0) are
`measurements/2026-09-24-emmc-sdhci/README.md`.

## Hardware (what matters to us)

| | |
|---|---|
| SoC | SpaceMiT K1: 8 RV64 harts in two clusters (RVA22 + vector), IMG B-series GPU, display controller (`spacemit,dpu`) with HDMI and MIPI-DSI, DWC3/xHCI USB3, USB2 OTG, two GbE MACs, SDIO |
| RAM | DDR, **0-based** (OpenSBI loads at `0x0`, U-Boot at `0x0020_0000` — measured 2026-09-21 from the vendor FIT images); QEMU virt is `0x8000_0000`-based — nothing in our code may care (Block 1 B1.4) |
| Storage | eMMC (ours) + microSD (the stock system lives there today); M.2 PCIe |
| Debug UART | 3-pin header **UART0**: GND / RX / TX, **3.3 V TTL**, **115200 8N1**. Needs a USB-UART adapter — none is connected on the desk yet (2026-09-21) |
| USB2 OTG port | the download/flash port; the stock system exposes an adb gadget there |
| DIP switches 1+2 | boot device selection; factory default = microSD first, then eMMC |

## USB identities (vendor id `361c` in every mode)

| ID | Mode | Seen |
|---|---|---|
| `361c:0008` | stock system running, adb gadget | 2026-09-21 on the desk (`lsusb`: "Spacemit K1 ADB") |
| `361c:1001` | boot-ROM download mode **and** U-Boot fastboot mode (`fastboot devices` reports `dfu-device DFU download` for the boot ROM) | from the vendor/community write-ups; measured at T2's `--stage-only` run |

The udev rule `config/udev/71-nexus-board.rules` (installed by `make initial-setup`, step 6/7)
matches the vendor id, so both modes work without sudo once you are in the serial group
(`uucp` on Arch, `dialout` on Debian/Ubuntu/Fedora — log out and in after the install).

## Entering download mode

- A **new/empty eMMC** enters it by itself when the boot ROM finds no boot media.
- An installed system: **hold the download key (labelled `FDL`) while resetting/powering the
  board**, or type `fastboot usb 0` at the U-Boot prompt over the serial console.
- `just board-devices` tells you which mode the board is in.

## The flash path (plain `fastboot`; the vendor's GUI flasher is not needed)

The vendor's own recipe (`fastboot.yaml` inside the release archive) and three independent
write-ups agree on this sequence. `scripts/board-flash.sh` stages the vehicle the same way
(the first four lines); what it writes is described below:

```
lsusb                                   # "DFU USB download gadget" = boot ROM → staging needed; "U-Boot USB download gadget" = already staged
fastboot stage factory/FSBL.bin ; fastboot continue    # SPL: DDR training, then waits again
fastboot stage u-boot.itb        ; fastboot continue    # vendor U-Boot in RAM = flashing mode
fastboot getvar product / blk-size / max-download-size …  # what --stage-only stops at (writes nothing; no `getvar all` exists)
fastboot flash gpt      partition_universal.json
fastboot flash bootinfo factory/bootinfo_sd.bin         # the universal table uses the SD header for eMMC too
fastboot flash fsbl     factory/FSBL.bin
fastboot flash env      env.bin
fastboot flash opensbi  fw_dynamic.itb
fastboot flash uboot    u-boot.itb
fastboot reboot
```

Our recipe writes none of the vendor's partitions and never lets the vehicle build a GPT
(`flash gpt <json>` types every partition basic data; its JSON regions compute offsets in 32
bits). It writes **our disk** instead — the user area, the backup GPT, boot0 — as raw partitions
declared in the vehicle's environment (`oem env:set fastboot_raw_partition_<name>:<start> <count>
[mmcpart <n>]`), each sized before it is written, from the plan `just board-image` makes (see "The
image for this board"). The stock system on the microSD card is not touched (the boot ROM tries
the card first).

Vendor pieces: `just board-inputs` (→ `scripts/fetch-board-inputs.sh`) fetches them pinned;
`resources/board/bpi-f3/PROVENANCE.md` lists versions, hashes and licenses.

## The board's device tree (ours)

`config/board/bpi-f3/board.dts` is the tree our boot chain hands to the kernel (RFC-0098 C1):
written by hand from the mainline SoC description (GPL-2.0 OR MIT, used under MIT) and the
measured facts, only the nodes we consume. `dtc -p 512` compiles it with headroom for nxboot's
`/chosen` writes; the host tests of `source/libs/nexus-fdt` read the compiled golden
(`tests/goldens/bpi-f3.dtb`) next to QEMU's dumped `virt.dtb`. On every boot the kernel prints
what it read: `KSELFTEST: platform from fdt ok (uart=… plic=… tb=…Hz harts=… chosen.slot=…)`.

## Recipes

| Recipe | Does |
|---|---|
| `just board-devices` | board mode (`stock` / `download` = boot ROM / `fastboot` = U-Boot in RAM / none) + serial adapter; exit 3 when no board |
| `just board-inputs` | fetch + verify the pinned vendor boot pieces (~250 MB download, once) |
| `just board-serial [PORT]` | picocom on the adapter at 115200 8N1, log tee'd to `build/logs/board--<ts>/uart.log` (`build/logs/latest-board`) — the same file shape the QEMU marker tools read |
| `just board-flash --stage-only` | boot ROM → SPL → U-Boot in RAM, prints the board's variables, writes nothing (the safe first contact) |
| `just board-image` | the board's disk (the eMMC's user area + boot0) and its flash plan, from the last OS build |
| `just board-flash --plan DIR` | the above, then write the flash plan to the eMMC: each region a raw partition, its size checked first (asks first) |
| `just board-flash --verify DIR` | read every region of the plan back from the stock system over adb and compare digests |
| `just board-log` | the boot trace (RFC-0107) without a serial adapter: the stock system (microSD in, board reset) pulls the eMMC's `trace` partition over adb; the latest boot → `build/logs/board--<ts>/uart.log`, every kept boot → `trace-all.log`. Exit 4 = the eMMC has no trace partition, 5 = no boot reached the loader's disk step since the disk was written (the chain stopped before nxboot, or nxboot found no boot disk) |
| `just board-ack MARKER=<name>` | append `board-visual: <name>` to the current board log — a human check becomes a marker the manifest can require (`TASK-0327B`) |

## Boot-ROM / SPL facts for Block 1 (measured from the vendor pieces, 2026-09-21)

- `bootinfo` is an 80-byte header at offset 0 of the boot medium (magic `f0 14 07 b0`, media tag
  `SDC` / `eMMC`, then offsets; CRC at `0x40`). The vendor's universal GPT places `fsbl` at
  128 KiB, `env` at 384 KiB, `opensbi` at 1 MiB, `uboot` at 2 MiB.
- `fw_dynamic.itb` = OpenSBI `fw_dynamic` FIT, load address `0x0`. `u-boot.itb` = FIT with
  image `uboot` (load `0x0020_0000`) and one DTB per vendor board variant (`k1-x_deb1`,
  `k1-x_MINI-PC`, `k1-x_MUSE-N1`, …); the SPL picks the configuration. **Which DTB reaches the
  next stage in `a1` is Block 1's R1 measurement**, and the `uboot` FIT image is the slot
  nxboot will occupy (ADR-0066).
- SPL links at `0xc080_1000` (SRAM `0xc080_0000`, 4 KiB header); DDR controller at
  `0xc000_0000`.

## The image for this board (TASK-0260 P1/P2)

The eMMC boots from its boot0 hardware partition: the boot-ROM header at 0 and the SPL at `0x200`.
The SPL then loads `opensbi` and `uboot` from the user area by name (the vendor's U-Boot source;
`docs/board/measurements/2026-09-26-boot-medium/`). `just board-image` builds, from the last OS
build (`config/board/bpi-f3/image.toml`):

- `build/board/bpi-f3/nexus.img` — the eMMC's user area byte for byte, a sparse file of its
  30 535 680 sectors: the protective MBR in sector 0, our GPT with `opensbi` and `uboot` as
  partitions 1–2 and our volumes from 4 MiB, the backup GPT at the last sector;
- `nexus.img.boot0` — boot0: the eMMC boot-ROM header (checked as the boot ROM checks it) and the
  SPL at the offset it names;
- `flash/` — the flash plan (`nx image flash-plan`): the raw regions and their digests.

`uboot` holds the loader's FIT once TASK-0260B builds it (`scripts/board-image.sh --fit FILE`).
Then `just board-flash --plan build/board/bpi-f3/flash` writes it. Each region is declared in
the vehicle as a raw partition and its size read back before it is written, and the board then
boots its microSD. `just board-flash --verify build/board/bpi-f3/flash` reads every region back
over adb. Done on the desk board on 2026-09-26: every region was exact, and the stock kernel reads
our nine partitions without a warning.

## Measured against the boot ROM (2026-09-21, `just board-flash --stage-only`)

| Step | Observed |
|---|---|
| download mode | `lsusb`: `361c:1001 DFU USB download gadget`; `fastboot devices`: `dfu-device DFU download`; `getvar version-brom` → `1.0` |
| `stage factory/FSBL.bin` (197 KB) | 28 ms; `continue` 7 ms; DDR training + USB re-enumeration inside 1 s |
| `stage u-boot.itb` (1936 KB) | 51 ms; `continue`; the board re-enumerates as `361c:1001 U-Boot USB download gadget`, `fastboot devices`: `???????????? Android Fastboot` |
| whole `--stage-only` | 4.5 s, nothing written |
| U-Boot variables | `product k1-x` · `version 0.4` · `version-bootloader U-Boot 2022.10spacemit-gdcdcab9e9-dirty` · `serialno 7cc8e2bef8fb` (same as the adb gadget) · `blk-size universal` · `mtd-size NULL` · `max-download-size 0x10000000` · `current-slot a` · `is-userspace no` |
| not implemented | `getvar all` (both stages), `slot-count`, `secure`, `unlocked`; every `partition-size:*` → "invalid partition or device" = the eMMC has never been flashed |
| **the trap** | `getvar version-brom` is NOT a discriminator: the vendor U-Boot answers it too. A second `--stage-only` run against the U-Boot already in RAM therefore staged the SPL again; U-Boot's `continue` then "resumed boot" and the board **vanished from USB** (no `361c` device for minutes) — power cycle needed. `board-devices --mode` now tells `download` (product string "DFU USB download gadget") from `fastboot` ("U-Boot USB download gadget"), and `board-flash` stages only in `download` mode and verifies the mode after staging (re-proven after a power cycle: boot-ROM run 5.2 s, second run skips staging in 0.5 s) |

`blk-size` is what the vendor recipe uses to pick `partition_{blk-size}.json`; our recipe does not
use it. `board-flash --plan` asserts product `k1-x` and a `max-download-size` of at least the
plan's chunk (256 MiB: the plan's chunks are exactly that). Leaving the board in U-Boot's fastboot mode is harmless; `fastboot
reboot` boots the microSD system again — its adb gadget (`361c:0008`) reappears after ~65 s (measured), so
a lane waiting for it must allow that long.

## Pitfalls

- `sudo make initial-setup` breaks the setup (everything would be created for root) — run it as
  yourself; the scripts ask for sudo where they need it.
- `fastboot` sees nothing: the board is in stock mode (`361c:0008`) — hold `FDL` at reset.
- Permission denied on the port: the serial-group membership is not effective yet — log out and in.
- The `!` shell of an editor/agent has no TTY for the sudo password (`sudo -v` in a terminal first).
