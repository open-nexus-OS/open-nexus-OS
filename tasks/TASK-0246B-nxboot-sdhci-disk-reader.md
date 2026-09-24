---
title: TASK-0246B nxboot reads the boot medium through the same readers as `blkd` — virtio and SDHCI (on the board, and behind QEMU's PCI host) — and names it in `/chosen`
status: Draft (seeded 2026-09-22 at Block 1 P0 as the B part of TASK-0246; recut 2026-09-24 at TASK-0246 P0 after measuring)
owner: @runtime @reliability
created: 2026-09-22
updated: 2026-09-24
depends-on:
  - tasks/TASK-0246-bringup-rv-virt-v1_1a-host-virtio-blk-image-builder-deterministic.md (P2 SDHCI core, P3 PCI planner)
follow-up-tasks:
  - tasks/TASK-0260B-nxboot-as-fit-payload-chosen-node.md
links:
  - Decisions: docs/adr/0066-boot-chain-on-hardware-nxboot-as-fit-payload.md, docs/adr/0067-one-block-owner-backend-selected-by-fdt.md (amended 2026-09-24)
  - Contract: docs/rfcs/RFC-0098-board-support-contract-fdt-truth-boot-chain.md (C3 PCI source, C5 boot disk)
  - nxboot today: source/boot/nxboot/src/virtio.rs (`VirtioDisk::probe`), src/flow.rs (BSB → slot → GPT → NXBD)
  - Measurement: docs/board/measurements/2026-09-24-emmc-sdhci/README.md
  - Playbook: CLAUDE.md
---

## Context (measured 2026-09-24)

nxboot's disk reader is `virtio.rs`, a virtio-blk MMIO reader. On the board the boot medium is
the eMMC behind the K1's SDHCI host; on QEMU the SDHCI stand-in is `sdhci-pci` behind the ECAM
host with an UNASSIGNED BAR — nothing before nxboot enumerates PCI. On QEMU no SPL runs before
nxboot (OpenSBI hands over directly); on the board the vendor SPL read the FIT from the eMMC,
so the host is clocked there — but its controller and card state are the SPL's, not a contract.
The OS must know which of the board's three SD hosts (and a stock SD card) holds the system
disk.

## Goal

`source/boot/nxboot/src/disk/{mod,virtio,sdhci}.rs`: a read-only, one-block-at-a-time
`DiskRead` seam with two backends chosen from the device tree (`nexus-fdt`): the virtio reader
and an SDHCI reader built on TASK-0246's `sdhci` core in PIO mode (no DMA, no interrupts —
nxboot has none), which **initialises the card itself** (reset, CMD0 → … → transfer state at a
conservative clock) and relies on no predecessor's state, so QEMU and the board take one path.
On QEMU the SDHCI function is found through TASK-0246's PCI planner (the same assignment init
sees). nxboot writes **`/chosen/nexus,boot-disk`** — the node path, or the PCI function behind a
host node — for the device it read the boot volume from; init grants exactly that one to `blkd`
(RFC-0098 C5). The rest of `flow.rs` is untouched.

## Non-Goals

Writing, ADMA, the command queue, HS400 (nxboot reads a few MiB at HS52 or below); SD-card boot
(the boot ROM tries SD first; our chain boots from eMMC, a stock SD card in the slot boots the
stock system — the desk fallback by design).

## Definition of Done

Both backends host-tested (the SDHCI reader against TASK-0246's model); `ci-os-sdhci` boots
through nxboot's SDHCI reader and `init` grants the device named in `/chosen`; on the board
`nxboot: platform=<compatible> slot=<a|b>` (TASK-0260B); ADR-0066/0067 consequences recorded.
