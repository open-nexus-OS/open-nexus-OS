<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Reference board `bpi-f3` — vendor boot pieces: provenance and licenses

`scripts/fetch-board-inputs.sh` downloads the vendor release archive below ONCE, verifies it
against the pinned SHA-256, extracts the pieces listed here into `vendor/` (gitignored — never
committed, never linked to our code) and verifies each against its own pin. This file is the
human copy of those pins; the script is the executable one. Update both together.

## Archive (pin of 2026-09-21)

| Field | Value |
|---|---|
| Release | vendor OS "Minimal" **v2.3.5**, SoC family `k1x` |
| URL | `http://archive.spacemit.com/image/k1/version/bianbu/v2.3.5/Bianbu-Minimal-K1-V2.3.5-20260601180942.zip` |
| Size | 263 751 151 bytes |
| Vendor MD5 (published next to the archive) | `b814512a52f31f59e879f8a2fc35b647` — verified on fetch |
| **SHA-256 (our pin)** | `364508121016d452295105894c493362dac594c5b3f61e162b782f46885d9bed` |

## Pieces

| Path in archive | SHA-256 | What it is | License of the program |
|---|---|---|---|
| `factory/FSBL.bin` | `980c0bca…9fad` | First-stage loader (U-Boot SPL build: DDR training, loads the FIT stages). Runs from SRAM `0xc080_0000`. | GPL-2.0-or-later (U-Boot); vendor build of open source |
| `factory/bootinfo_sd.bin` | `f339e3e5…3cb6` | 80-byte boot-ROM header (`SDC` tag) — the vendor's universal partition table uses THIS one for eMMC too | vendor data blob |
| `factory/bootinfo_emmc.bin` | `5433619a…3d07` | 80-byte boot-ROM header (`eMMC` tag) — kept for Block 1's measurement (R1) | vendor data blob |
| `fw_dynamic.itb` | `ef46cb0b…a52d` | OpenSBI `fw_dynamic` as a FIT, load address `0x0` (DRAM is 0-based on this SoC) | BSD-2-Clause (OpenSBI) |
| `u-boot.itb` | `124c30cb…33ee` | Vendor U-Boot proper as a FIT (image `uboot` at `0x0020_0000` + one DTB per vendor board variant) — the host-side FLASHING vehicle, runs in RAM during `just board-flash` | GPL-2.0-or-later (U-Boot) |
| `env.bin` | `f91f8d85…1f90` | U-Boot environment (16 KiB) | vendor data |
| `partition_universal.json` | `568d9848…568e` | The vendor flasher's GPT: `bootinfo` 0K/512 (hidden), `fsbl` 128K/256K, `env` 384K/64K, `opensbi` 1M/1M, `uboot` 2M/2M, `bootfs` 4M/256M, `rootfs` 260M/rest | vendor data |
| `fastboot.yaml` | `95642b76…5278` | The vendor flasher's own recipe: `getvar version-brom` → `stage FSBL.bin` → `continue` → `stage u-boot.itb` → `continue` → `getvar mtd-size`/`blk-size` → multi-flash by partition json. `scripts/board-flash.sh` follows it step for step. | vendor data |

Full hashes are in `scripts/fetch-board-inputs.sh` (`PIECES`).

## What we do and do not do with them

- **Flash vehicle only:** SPL + U-Boot run in the board's RAM to write the eMMC; the vendor
  U-Boot is never part of a booted Open Nexus OS system (ADR-0066, Block 1).
- **Stages that may stay in the booted chain** until Block 1 decides: the SPL (DDR init) and
  OpenSBI — each a separate program in its own partition, fetched at setup time, source pinned by
  the vendor's public repositories for the same release; nothing here is linked into our binaries.
- **Not fetched:** `bootfs.ext4`, `rootfs.ext4` (the vendor OS) — we never write them.
- Firmware for devices (GPU, WiFi) is a different matter with its own provenance gate
  (`resources/firmware/<device>/`, hardware rule 3 in `tasks/IMPLEMENTATION-ORDER.md`).
