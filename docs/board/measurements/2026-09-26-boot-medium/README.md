<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# The boot medium — 2026-09-26 (TASK-0260 P0)

What a medium this board boots from looks like, byte for byte, what the vendor flash vehicle can
and cannot write, and the state of the eMMC we will write. Read from the stock system over `adb`
(root; the stock microSD boots, the eMMC is idle) and from the pinned vendor boot pieces
(`resources/board/bpi-f3/PROVENANCE.md`). Nothing was written.

Files: `sd-sector0-1.txt` (the microSD's sectors 0–1), `sd-gpt-entries.txt` (its GPT entries),
`emmc-state.txt` (the eMMC), `vendor-vehicle-strings.txt` (the evidence strings of the SPL and
of the vendor U-Boot the flash recipe stages into RAM).

## Sector 0 is shared: the boot-ROM header and the protective MBR (`sd-sector0-1.txt`)

- Bytes 0–79: the boot-ROM header, byte-identical to the pinned `bootinfo_sd.bin` (magic
  `0xb00714f0`, tag `SDC`, the SPL's offset 128 KiB, CRC `0x36ccf84d`). The vendor partition table
  writes only these 80 bytes (`"holes": "(80;512)"`).
- Bytes 446–461 and 510–511: a **protective MBR** — one entry of type `0xee` from LBA 1 over the
  whole disk (size `disk − 1` = 62 423 039), signature `0x55aa`. Both live in one sector.
- Sector 1: the GPT header; its backup lies at the disk's **last** sector (62 423 039); 128
  entries of 128 bytes from LBA 2; first usable LBA 34.

Why the protective MBR matters: U-Boot's GPT driver — and with it the SPL, which finds its next
stages through the GPT — recognises a GPT only behind a valid protective MBR (upstream U-Boot
2022.10, `disk/part_efi.c`: `part_test_efi` → `is_pmbr_valid` wants the `0x55aa` signature and
an entry of type `0xee` starting at LBA 1). A disk without one has no partitions for the SPL.

## The GPT the stock medium carries (`sd-gpt-entries.txt`)

| # | name | offset | size |
|---|---|---|---|
| 1 | `fsbl` | 128 KiB | 256 KiB |
| 2 | `env` | 384 KiB | 64 KiB |
| 3 | `opensbi` | 1 MiB | 1 MiB |
| 4 | `uboot` | 2 MiB | 2 MiB |
| 5 | `bootfs` | 4 MiB | 256 MiB |
| 6 | `rootfs` | 260 MiB | to the end |

Every entry carries the generic basic-data type (`ebd0a0a2-b9e5-4433-87c0-68b6b72699c7`) — the
vehicle's partition JSON has no type field (neither this board's nor the next SoC generation's
public one). `bootinfo` is no entry: it is sector 0.

## How the SPL finds OpenSBI and the payload (`vendor-vehicle-strings.txt`, SPL)

(Answered by the addendum: by name, as the SPL's configuration names the two partitions.)

The SPL carries the names `opensbi` and `uboot` and upstream's `spl: partition error` — the
message of the path that looks a partition up in the GPT (upstream: by number). Whether this
build looks up by name or by number is not visible in strings; a disk that keeps the four head
partitions' **names, numbers and offsets** satisfies either, and the board run shows which
(TASK-0260 P3). The payload's slot keeps the SPL's name `uboot` whatever it holds. The SPL also
selects a FIT configuration by name (`Boot from fit configuration %s`) — TASK-0260B's question.

## What the flash vehicle can write (`vendor-vehicle-strings.txt`, U-Boot)

- `fastboot flash gpt <json>` builds the GPT itself: `gpt write mmc 0 $partitions` from the JSON,
  then a second string form ("Both GPT write methods failed"). Upstream's raw path for
  `flash gpt` ("updating MBR, Primary and Backup GPT(s)") is **absent**: our GPT bytes — our
  partition types — cannot reach the disk through `flash gpt`.
- Raw writes exist beside it: `flash bootinfo` is special-cased (checked against its CRC, never a
  GPT entry), the JSON's `hidden` regions are written at their offsets without an entry (this
  board's `bootinfo`; the next SoC generation's public table declares `fsbl`, `opensbi` and
  `uboot` hidden too), `fastboot_raw_partition_*` (upstream's raw regions by offset), sparse
  images (`write_sparse_image`), `flash mbr`. (`set_protective_mbr` is upstream's helper that
  `gpt write` calls — why the stock medium's sector 0 carries one.)
- Read-back exists: `oem read:<part> [offset]` + `upload`.

Which of these this vehicle accepts for a region other than `bootinfo`, and how large a sparse
write may be, is measured in U-Boot fastboot mode before anything is written (TASK-0260 P2).

## The eMMC (`emmc-state.txt`)

30 535 680 sectors (14.56 GiB), sectors 0–33 all zero, no partition: never flashed. The hardware
boot partitions are read-only in the stock system. The boot ROM tries the microSD first, so a
wrong eMMC write cannot keep the desk board from booting its stock system; download mode
recovers the eMMC.

## What this changes for TASK-0260 (read with the ledger)

(Superseded in part by the addendum at the end, the same day: the eMMC boots from its boot0
hardware partition — header and SPL there — and the SPL finds `opensbi` and `uboot` by name,
so the user area's head is those two partitions and sector 0 carries the protective MBR alone.)

1. The layout carries the head as GPT partitions 1–4 with the vendor's names, numbers and
   offsets; our volumes follow from 4 MiB.
2. Every image carries a protective MBR (RFC-0089 §2's "GPT-only, no protective MBR" amendment of
   2026-08-25 does not survive the board); the board image adds the boot-ROM header in bytes
   0–79 of the same sector.
3. The GPT describes the device it lands on: its backup at the device's last sector.
4. The flash path writes our bytes; the vehicle's JSON GPT (every partition basic data) is never
   the disk's truth.

## Addendum, later the same day (TASK-0260 P2): how the eMMC boots, and the first write

Read in the vendor's public U-Boot 2022.10 tree: the fastboot flash driver
(`drivers/fastboot/fb_<vendor>.c`, `fb_mmc.c`, `fb_command.c`), its header and the board's
defconfig. The facts are quoted there; no code is copied.

- **The eMMC boots from its boot0 hardware partition, not from the user area.** `flash bootinfo`
  writes an eMMC header (media tag `eMMC`, the SPL at `0x200`, limit `SPL_SIZE_LIMIT + 0x1100`)
  into hardware partition 1 at 0, and `flash fsbl` (`CONFIG_FASTBOOT_MMC_BOOT1_NAME="fsbl"`)
  writes the SPL there at `0x200`. That is the pinned `bootinfo_emmc.bin`: the header layout is
  magic, version, media tag, page/block/total size, `spl0_offset` at `0x20`, `spl_size_limit` at
  `0x28`, and the CRC of 64 bytes at `0x40`. The stock microSD's header in sector 0 is the SD
  card's form (tag `SDC`, the SPL at 128 KiB). The DIP switches decide the device; by default the
  boot ROM tries the microSD first.
- **The SPL finds its stages by name:** `CONFIG_SYS_LOAD_IMAGE_PARTITION_NAME="opensbi"`,
  `CONFIG_SYS_LOAD_IMAGE_SEC_PARTITION_NAME="uboot"`. This answers "by name or by number".
  `fsbl` and `env` as user-area partitions serve the SD card's form only, so the layout drops them.
- **The vehicle's JSON regions compute byte offsets in 32 bits** (`combine_size * 1024` from an
  `int`, `%x`-formatted). They cannot reach the backup GPT at 14.5 GiB. A JSON of hidden regions
  only writes no GPT at all ("maybe there doesn't have gpt/mtd partition, should not return
  fail").
- **The vehicle's environment is writable** (`oem env:set <name>:<value>`), and the vehicle keeps
  upstream's raw partitions (`fastboot_raw_partition_<name>` = `<start> <count> [mmcpart <n>]`,
  64-bit block addresses).

Measured in U-Boot fastboot mode, nothing written (`fastboot-probes.txt`): a raw partition
declared through the environment answers `getvar partition-size` exactly. That holds for 16
sectors at `0x3000`, 34 sectors at the eMMC's end (sector 30 535 646) and the whole boot0
(`mmcpart 1`: `0x400000`). With no GPT on the eMMC, the vehicle knows no `opensbi`.

The first write, with the user's go (`flash-transcript.txt`): the flash plan of our disk. That is
the user area's 373 MiB in two chunks, the backup GPT in the last 33 sectors, and boot0 (header +
SPL). Each region was declared, sized and flashed: 256 MiB in 16.7 s, 117 MiB in 8.0 s. Then the
board reset into the stock system on the microSD.

Read back from the stock system (`readback.txt`): every region's SHA-256 matches the plan. The
kernel reads `mmcblk2: p1 … p9` with no GPT warning, and lists our names, offsets, type GUIDs and
distinct unique GUIDs.
