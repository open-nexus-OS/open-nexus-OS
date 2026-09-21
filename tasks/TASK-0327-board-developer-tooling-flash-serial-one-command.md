---
title: TASK-0327 Board developer tooling: one command installs the flash/serial tools on Ubuntu, Arch and Fedora + `just board-*`
status: Draft (seeded 2026-09-21 by the hardware fast track — `tasks/IMPLEMENTATION-ORDER.md`; rewritten to end state at its block's P0)
owner: @devx @runtime
created: 2026-09-21
depends-on: []
follow-up-tasks: []
links:
  - Execution order (the lane this belongs to): tasks/IMPLEMENTATION-ORDER.md
  - Playbook: CLAUDE.md
  - Setup spine: Makefile (`initial-setup`), scripts/install-deps.sh, scripts/check-deps.sh, scripts/fetch-inputs.sh
  - Boot-ROM/flash facts: docs/board/ (written by T2)
---

## Origin

No ledger existed for host developer tooling (0262/0263 are repo hygiene; `TRACK-CONSOLE-AND-TOOLCHAINS` is the in-OS console). Minted for Block 0 of the hardware fast track.

## Context

`make initial-setup` / `scripts/install-deps.sh` bootstrap a QEMU-only workstation; nothing installs `fastboot`, a serial terminal, `mkimage`, `dtc` or a udev rule for the board's USB IDs (`361c`), and no `just` recipe talks to a board. The reference board is on the desk, connected over USB-C.

## Goal

One command on a fresh Ubuntu/Debian, Fedora or Arch box leaves the developer able to detect the board, open its debug UART, flash the image `nx image` builds over the boot-ROM download mode + fastboot, and run the board proof lane (`TASK-0327B`, seeded at P0).

## Non-Goals

Board support itself (Block 1), any change to what the OS image contains, vendor GUI flashers (plain `fastboot` is the protocol).

## Packages (from the order file; the P0 rewrite fixes them)

- **T0 Paper** — this ledger to end state; hygiene: the second `TASK-0081` file → free `0209`.
- **T1 Packages** — `BOARD` candidate list per family in `install-deps.sh` (android-tools, serial terminal, dtc, u-boot-tools, gptfdisk, usbutils, xz), udev rule `config/udev/71-nexus-board.rules` + group membership, `check-deps.sh` capability rows. Gate: `make doctor` green on all three families, `fastboot devices` sees the board in download mode.
- **T2 Recipes** — `just board-devices`, `board-serial` (tee to `build/logs/board--<ts>/uart.log`), `board-flash` (stage vendor FSBL + U-Boot from `resources/board/<board>/`, `fastboot flash` our partitions), `board-ack MARKER=…`; `docs/board/<board>.md`. Gate: end to end against the connected board; serial log parsed by `verify-uart`.
- **T3 Board lane** (`TASK-0327B`, seeded at P0) — `scripts/board-test.sh`, profiles `board-headless`/`board-visible`, `just board-test`, opt-in in `test-all` under `NEXUS_BOARD=1`.

## Constraints / invariants (hard requirements)

- **No fake success**: no `*: ready` / `SELFTEST: * ok` markers unless the real behavior happened; a
  human-visible board check is an operator-acked `board-visual:` marker, never prose.
- **The FDT is the one hardware truth**: no address, IRQ, frequency or hart count outside the parser.
- **Firmware blobs** only under `resources/firmware/<device>/` with provenance + license and a gate.
- **Vendor kernel code is reference only**; openly licensed userspace driver code may be ported.
- **Rust hygiene**: no `unwrap`/`expect` on untrusted input; `forbid(unsafe_code)` in userspace crates
  except the one documented MMIO/DMA seam per driver.

## Red flags / decision points

- **RED**: the measurements named in the order file (R-items) are done BEFORE the end-state rewrite.
- **YELLOW**: —
- **GREEN**: —

## Definition of Done

Filled at P0 from the order file's gates: host tests → QEMU profile → board lane marker(s), old
mechanism deleted with a gate against its return, docs sweep (CHANGELOG, board, RFC/ADR status).
