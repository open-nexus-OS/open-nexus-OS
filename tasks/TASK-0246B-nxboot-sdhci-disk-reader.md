---
title: TASK-0246B nxboot reads the boot medium through the same two readers as `blkd` — virtio on QEMU, SDHCI on the board
status: Draft (seeded 2026-09-22 at Block 1 P0 as the B part of TASK-0246)
owner: @runtime @reliability
created: 2026-09-22
depends-on:
  - tasks/TASK-0246-bringup-rv-virt-v1_1a-host-virtio-blk-image-builder-deterministic.md
follow-up-tasks:
  - tasks/TASK-0260B-nxboot-as-fit-payload-chosen-node.md
links:
  - Decisions: docs/adr/0066-boot-chain-on-hardware-nxboot-as-fit-payload.md, docs/adr/0067-one-block-owner-backend-selected-by-fdt.md
  - nxboot today: source/boot/nxboot/src/virtio.rs (`VirtioDisk::probe`), src/flow.rs (BSB → slot → GPT → NXBD)
  - Playbook: CLAUDE.md
---

## Context

nxboot's disk reader is `virtio.rs` — a virtio-blk MMIO reader at the QEMU window. On the
board the boot medium is the eMMC behind an SDHCI host (TASK-0246 measured it). nxboot must
find the GPT, the BSB and the slot's NXBD on that medium with the same flow.

## Goal

`source/boot/nxboot/src/disk/{mod,virtio,sdhci}.rs`: a read-only, one-block-at-a-time
`DiskRead` seam with two backends selected by the FDT node nxboot finds (`nexus-fdt`,
TASK-0244): the existing virtio reader and a minimal SDHCI reader (PIO, no ADMA, no
interrupts — nxboot has no IRQ plane; the SPL left the controller clocked and the card
initialised, nxboot re-reads CSD/ext-CSD and reads sectors). The rest of `flow.rs` is
untouched. Host tests over a mock bus; QEMU proof through the `ci-os-sdhci` profile of
TASK-0246 booting through nxboot; board proof = the first `nxboot: slot A` on the serial
console (TASK-0260B).

## Non-Goals

Writing, ADMA, card power-up from cold (the SPL did it), SD-card boot (the boot ROM order is
SD first; our chain boots from eMMC — a stock SD card in the slot boots the stock system, by
design the desk fallback).

## Definition of Done

Both backends host-tested; nxboot boots the sdhci QEMU profile; `nxboot: platform=<compatible>
slot=<a|b>` on the board; ADR-0066/0067 consequences recorded.
