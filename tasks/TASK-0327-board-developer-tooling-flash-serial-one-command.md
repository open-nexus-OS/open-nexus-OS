---
title: TASK-0327 Board developer tooling: one command installs the flash/serial tools on Ubuntu, Arch and Fedora + `just board-*`
status: In Progress (T0, T1 Done; T2 Recipes built 2026-09-21 — fetch pipeline + recipes proven on the desk box; gate open: the board's download mode (`--stage-only`) needs a hand on the FDL key)
owner: @devx @runtime
created: 2026-09-21
depends-on: []
follow-up-tasks:
  - tasks/TASK-0327B-board-proof-lane-serial-ladder-profiles.md
links:
  - Execution order (Block 0 of the hardware fast track): tasks/IMPLEMENTATION-ORDER.md
  - Playbook: CLAUDE.md
  - Setup spine this extends: Makefile (`initial-setup`, `doctor`), scripts/install-deps.sh, scripts/check-deps.sh, scripts/fetch-inputs.sh
  - The ADR anchor every setup script cites: docs/architecture/02-selftest-and-ci.md
  - Board notes (written by T2): docs/board/bpi-f3.md
  - Consumers: tasks/TASK-0260-provisioning-recovery-v1_0a-flashd-host-image-builder-flasher-protocol-deterministic.md (fastboot becomes THE flasher protocol at Block 1), tasks/TASK-0244-bringup-rv-virt-v1_0a-host-dtb-sbi-shim-deterministic.md (Block 1 starts on a box this task prepared)
---

## Origin

No ledger existed for host developer tooling (0262/0263 are repo hygiene; `TRACK-CONSOLE-AND-
TOOLCHAINS` is the in-OS console). Minted 2026-09-21 as Block 0 of the hardware fast track: the
reference board (BPI-F3 / SpaceMiT K1) is on the desk, connected over USB-C, and nothing on a
developer box can talk to it.

## Context (measured 2026-09-21 on the desk box, a Manjaro/Arch machine)

- `make initial-setup` → `scripts/install-deps.sh` bootstraps a QEMU-only workstation: per-family
  `CORE`/`GUI`/`OPTIONAL` candidate lists (`"a|b"`, first name the local index knows wins), adapters
  for apt/dnf/pacman, a rustup bootstrap, `ensure_cargo_tool` fallbacks. `scripts/check-deps.sh`
  verifies CAPABILITIES (`need_bin bin why fix`), never package names — the one post-condition that
  means the same on all three distros. Six numbered steps in the Makefile; `make doctor` = check-deps.
- **What the box has:** `dtc`, `lsusb`, `xz`. **What it lacks:** `fastboot`, `adb`, any serial terminal
  (`picocom`/`tio`/`minicom`), `mkimage`, `sgdisk`/`gdisk`; no udev rule for the board's vendor id
  `361c`; the user is in none of `uucp`/`dialout`/`plugdev`; **no USB-UART adapter is connected**
  (`/dev/ttyUSB*`, `/dev/ttyACM*`, `/dev/serial/by-id` all absent). So the desk box IS the "fresh box"
  this task is for, and the serial half of the ladder needs a 3.3 V TTL adapter on the board's 3-pin
  UART0 debug header (GND/RX/TX, 115200 8N1) before T2 can be proven end to end.
- **What the board shows:** `lsusb` → `361c:0008 Spacemit K1 ADB` — the stock system is up (from its
  microSD card) with an ADB gadget. In boot-ROM download mode the board enumerates as **`361c:1001`**
  (fastboot reports it as `dfu-device DFU download`); a new/empty eMMC enters that mode by itself, an
  installed system needs the **FDL button held during reset** (or `fastboot usb 0` at the U-Boot
  prompt over serial). The USB2 OTG port is the download port.
- **The vendor flash path, from three independent write-ups (verbatim commands):**
  ```
  fastboot stage factory/FSBL.bin && fastboot continue      # SPL: DDR training, then it waits again
  sleep 1
  fastboot stage u-boot.itb        && fastboot continue      # U-Boot proper in RAM = fastboot mode
  fastboot flash gpt      partition_universal.json
  fastboot flash bootinfo factory/bootinfo_emmc.bin          # one source says bootinfo_sd.bin even for eMMC — measure (R13)
  fastboot flash fsbl     factory/FSBL.bin
  fastboot flash env      env.bin
  fastboot flash opensbi  fw_dynamic.itb
  fastboot flash uboot    u-boot.itb
  fastboot flash bootfs   bootfs.ext4 ; fastboot flash rootfs rootfs.ext4 ; fastboot reboot
  ```
  The pieces come from the vendor release `.zip` (not `.img.zip`) under
  `archive.spacemit.com/image/k1/version/bianbu/<version>/` (versions seen: v2.0 … v2.3.5 dated
  2026-08-27, v3.0.1 dated 2025-08-15). Only **FSBL.bin, u-boot.itb, fw_dynamic.itb, bootinfo_*.bin
  and the partition json** matter to us: SPL and U-Boot are the host-side flashing vehicle (they run
  in RAM), OpenSBI + SPL are the two vendor stages that stay in the booted chain until Block 1 decides
  otherwise (ADR-0066). Licenses: OpenSBI BSD-2-Clause, U-Boot SPL GPL-2.0 (a separate program in its
  own partition, fetched at setup time with its source pinned — never linked to our code).
- SPL facts for Block 1 (from the DRAM-init write-up): SRAM at `0xc080_0000`, SPL links at
  `0xc080_1000` behind a 4 KiB header, DDR controller at `0xc000_0000`, DRAM density reported by the
  training log (`4096 MB` on the 4 GB board) — the board's RAM is 0-based, unlike QEMU virt.
- Package names verified: **Arch** `android-tools` 37.0.0, `picocom` 3.1, `uboot-tools` 2026.07, `dtc`,
  `gptfdisk`, `usbutils` (all in the index; `android-udev` exists but carries no `361c` rule — we ship
  our own). **Debian/Ubuntu** `fastboot`\|`android-tools-fastboot` (+ `adb`\|`android-tools-adb`),
  `u-boot-tools`, `device-tree-compiler`, `picocom`, `gdisk`, `usbutils`. **Fedora** `android-tools`,
  `uboot-tools`, `dtc`, `picocom`, `gdisk`, `usbutils`. The candidate-list mechanism absorbs the rest.

## Goal

One command on a fresh Ubuntu/Debian, Fedora or Arch box (`make initial-setup`, step 7/7 "board
tools") leaves the developer able to: see the board in either mode (`just board-devices`), open its
debug UART with the log tee'd like a QEMU run (`just board-serial`), flash the image `nx image`
builds over the boot-ROM download mode + fastboot (`just board-flash`), and acknowledge a
human-visible check as a marker (`just board-ack`). The board proof lane on top of that is
`TASK-0327B`.

## Non-Goals

Board support itself (Block 1: FDT, HAL, SDHCI, boot chain, display), any change to what the OS image
contains, vendor GUI flashers (plain `fastboot` is the protocol; the vendor's GUI flasher is not
installed), USB gadget/device mode on our OS (`TASK-0261`, parked), automated power cycling
(no relay on the desk; the lane waits for a manual reset where SBI SRST cannot help).

## End state (binding)

**Packages — `install-deps.sh`:** a fourth list per family, `BOARD`, installed by default and skipped
with `--no-board` / `BOARD=0` (mirrors `GUI`): fastboot + adb, a serial terminal, `mkimage` (FIT
images, Block 1), `dtc` (board dts → dtb, Block 1), `sgdisk` (image verification), `usbutils`, `xz`.
Like `GUI`, an unresolvable `BOARD` name warns, never blocks the setup.

**Permissions — no sudo at flash time:** a udev rule `config/udev/71-nexus-board.rules` (vendor
`361c`, both product ids we know — `0008` gadget, `1001` download/fastboot — `TAG+="uaccess"`,
`MODE="0660"`, `GROUP="plugdev"|"uucp"` per family) installed by `install-deps.sh` through the
existing sudo adapter into `/etc/udev/rules.d/` (+ `udevadm control --reload`), and the invoking user
added to the serial group of the family (`dialout` on Debian/Fedora, `uucp` on Arch) — with the
honest note that group membership needs a re-login, printed by `check-deps.sh` until it is effective.

**Verification — `check-deps.sh` "Board tools" section:** `need_bin fastboot`, `need_bin mkimage`,
`need_bin dtc`, `need_bin sgdisk`, one of `picocom|tio|minicom` (soft: any), the udev rule present and
containing `361c` (soft otherwise: "flash needs sudo"), serial-group membership effective (soft:
"re-login"), and an informational line for a connected board (`361c:0008` = stock gadget,
`361c:1001` = download mode) and for a serial adapter (`/dev/serial/by-id/*`). `BOARD=0` skips the
section, like `GUI=0`. `make initial-setup` gets step **7/7 "Board tools"** and the header comment
its `BOARD=0` flag.

**Recipes — `justfile` + `scripts/board-*.sh`:**
- `just board-devices` — lists `361c` devices with their mode, and serial adapters; exit 3 when
  nothing is connected (the lane uses it as a precondition).
- `just board-serial [PORT]` — picocom at 115200 8N1 on the adapter (autodetected from
  `/dev/serial/by-id`, overridable), tee to `build/logs/board--<ts>/uart.log` so
  `verify-uart`/`just check-markers` read a board log exactly like a QEMU log.
- `just board-flash [IMAGE]` — precondition: board in download mode (`361c:1001`); stages the vendor
  FSBL + U-Boot from `resources/board/bpi-f3/` (fetched by `scripts/fetch-inputs.sh --board` with
  URL + SHA-256 + license pins, never committed), then `fastboot flash` of our partitions from the
  image `nx image` builds (the partition set is Block 1's `layout.rs` SSOT; until B1.6 lands the
  recipe flashes the vendor layout's `gpt/bootinfo/fsbl/env/opensbi/uboot` + our `boot-a/system-a`
  and says so in its banner — no fake "our chain" claim).
- `just board-ack MARKER=<name>` — appends `board-visual: <name>` to the current board log; the
  proof manifest declares the `board-visual:` markers the board profiles require (`TASK-0327B`).
- `docs/board/bpi-f3.md` — the boot-ROM sequence, download-mode entry, USB ids, serial pins, the
  partition set, the vendor-archive provenance, and the pitfalls measured in T2.

**Gates:** `NEXUS_FORCE_FAMILY=debian|fedora|arch scripts/install-deps.sh --print-packages` resolves the
`BOARD` tables on any host (the script's own stated test coverage — there is no per-family container
smoke in the tree; measured 2026-09-21), `bash -n` on every script, `make doctor` green on the desk box
with the new section; on the desk box:
`just board-devices` finds `361c:0008`, `fastboot devices` lists the board once it is put into
download mode, `just board-serial` shows the stock system's console (adapter required — see RED).

## Packages

- **T0 Paper (this)** — ledger to end state, host measurement above, hygiene: the second `TASK-0081`
  file moved to `TASK-0209`; `TASK-0327B` seeded.
- **T1 Packages + permissions + doctor — Done 2026-09-21** (`make initial-setup` as the user, then `make doctor`: fastboot/mkimage/dtc/sgdisk/picocom present, rule installed with group `uucp`, membership effective, board `361c:0008` seen; the run also found that `sudo make initial-setup` made the workspace check judge against root's home and propose chowning the checkout to root — the check now resolves `SUDO_USER` and refuses to create rustup/cargo/podman state as root) — `install-deps.sh` `BOARD` lists (+ `--no-board`),
  `scripts/install-board-access.sh` (udev rule from `config/udev/71-nexus-board.rules` with the family's
  serial group substituted + `usermod -aG`; idempotent; `--check`), `check-deps.sh` "Board tools"
  section, Makefile steps 1/7…7/7 (6/7 = board access, `BOARD=0` skips), README. Gate: `--print-packages`
  ×3 families, `make doctor` on the desk box shows the board line.
- **T2 Recipes + provenance + docs — built 2026-09-21.** `scripts/board-devices.sh` (mode
  `stock|download|other|none`, `--mode`, `--serial`; exit 3 without a board), `board-serial.sh`
  (picocom 115200 8N1, log tee'd to `build/logs/board--<ts>/uart.log` + `latest-board`; `--capture`
  for the lane), `board-ack.sh` (refuses names the manifest does not declare — the `board-visual:`
  names arrive with `TASK-0327B`'s `markers/board.toml`, so today it can only refuse), `board-flash.sh`
  (follows the vendor's own `fastboot.yaml` step for step: `getvar version-brom` decides whether to
  stage; `--stage-only` writes nothing and prints `getvar all`; the write path flashes the vendor
  BOOT VEHICLE only — GPT + bootinfo/fsbl/env/opensbi/uboot — with a prompt and a banner naming the
  chain), `fetch-board-inputs.sh` (vendor archive pinned by SHA-256, the eight pieces pinned each,
  python `zipfile` — no `unzip` on the box — cache under `vendor/.cache/`, all gitignored),
  `fetch-inputs.sh --board`, `just board-*` + help, `docs/board/bpi-f3.md`,
  `resources/board/bpi-f3/PROVENANCE.md`. **Measured from the vendor pieces (closes the YELLOW):**
  the vendor's universal partition table itself uses `factory/bootinfo_sd.bin` for eMMC; layout
  `bootinfo` 0K/512 hidden, `fsbl` 128K/256K, `env` 384K/64K, `opensbi` 1M/1M, `uboot` 2M/2M,
  `bootfs` 4M/256M, `rootfs` 260M/rest; `fw_dynamic.itb` loads at `0x0`, the vendor U-Boot at
  `0x0020_0000`, its FIT carries one DTB per vendor board variant (`k1-x_deb1`, …) — R1 material.
  Proven on the desk box: fetch from cache 0.8 s, second run a no-op, `--check --board` green,
  doctor row green, every recipe's precondition honest (stock mode → `board-flash` exit 3, no
  adapter → `board-serial` exit 3). **Open:** the `--stage-only` run against the boot ROM (R13:
  timing of `continue`, the `getvar all` set) needs the board in download mode — the FDL key.
  Former text: `scripts/board-devices.sh`, `board-serial.sh`, `board-flash.sh`,
  `board-ack.sh`, `fetch-inputs.sh --board` (pins + license files under `resources/board/bpi-f3/`,
  gitignored payloads), `docs/board/bpi-f3.md`. Gate: end to end against the connected board (R13
  measured here: `bootinfo_sd` vs `bootinfo_emmc`, stage sizes/timing, `continue` wait), serial log
  parsed by `verify-uart`.
- **T3 Board lane** — `TASK-0327B`.

## Constraints / invariants (hard requirements)

- **No fake success**: `board-flash` prints which chain it flashed (vendor bootloader stages + our
  volumes until Block 1's B1.6); a human-visible check is an operator-acked `board-visual:` marker,
  never prose.
- **No secrets, no sudo at flash time**: the udev rule is the permission model; scripts never call
  sudo except `install-deps.sh` through its existing adapter.
- **Vendor binaries are inputs, not sources**: fetched with URL + SHA-256 + license pins by
  `fetch-inputs.sh`, never committed, never linked; `fetch-inputs.sh --check` reports them.
- **Capabilities, not package names**, in `check-deps.sh` (its stated design).
- **Bash hygiene** as in the existing scripts: `set -euo pipefail`, no `ls | head` under pipefail
  (see `scripts/input-flood-lane.sh`), no `pgrep -f` self-matches from inline shells.

## Red flags / decision points

- **RED (blocks T2's serial proof):** no USB-UART adapter is connected to the desk box; the board's
  3-pin UART0 header is 3.3 V TTL, 115200 8N1. The flash half (fastboot over the USB2 OTG port) does
  not need it; the marker ladder does.
- **GREEN (was YELLOW, measured 2026-09-21):** the vendor's `partition_universal.json` uses `bootinfo_sd.bin`
  for eMMC too; the recipe follows the json, `bootinfo_emmc.bin` is kept only for Block 1's R1. The download key is
  labelled `FDL` on the board; an installed system does not enter download mode on its own.
- **YELLOW:** distro `android-udev` rule sets do not carry `361c`; our rule is not optional.
- **GREEN:** `fastboot` from `android-tools` speaks the boot-ROM's protocol (the vendor's "flashserver"
  is a modified fastboot; three write-ups flash with the plain one).

## Security considerations

N/A beyond permissions: the udev rule grants the logged-in user access to one vendor id; nothing
runs as root at flash time; no credentials are involved.

## Definition of Done

- `make initial-setup` on a fresh box of each family ends with a green `make doctor` whose "Board
  tools" section is green (adapter/board lines informational), and `BOARD=0` skips it.
- `just board-devices`, `board-serial`, `board-flash`, `board-ack` exist, are documented in
  `docs/board/bpi-f3.md`, and ran end to end against the desk board (flash of the current image,
  stock console on serial), with the R13 pitfalls recorded in the doc.
- `fetch-inputs.sh --board` fetches the pinned vendor pieces with their license files;
  `--check` reports them; payloads are gitignored.
- CHANGELOG entry, `README.md` quickstart line, `docs/testing/README.md` pointer to the board log
  location; `tasks/IMPLEMENTATION-ORDER.md` Block 0 row Done.
