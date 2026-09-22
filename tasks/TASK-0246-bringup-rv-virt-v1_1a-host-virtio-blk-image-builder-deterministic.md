---
title: TASK-0246 Block driver on hardware: SDHCI/eMMC at `BlockDevice`, and `virtioblkd` becomes `blkd` — the one block owner with an FDT-selected backend
status: Draft (recut 2026-09-22 to the end state — Block 1 B1.5 of the hardware fast track; was "RISC-V Bring-up v1.1a: virtio-blk frontend core + packagefs image builder", whose subjects shipped as TASK-0314 and TASK-0260)
owner: @runtime @kernel-team
created: 2025-12-29
updated: 2026-09-22
depends-on:
  - tasks/TASK-0245-bringup-rv-virt-v1_0b-os-kernel-uart-plic-timer-uartd-selftests.md
  - tasks/TASK-0245B-board-support-v1c-soc-clock-reset-pinmux-power-from-fdt.md
  - tasks/TASK-0286-kernel-memory-accounting-v1-rss-pressure-snapshots.md
follow-up-tasks:
  - tasks/TASK-0246B-nxboot-sdhci-disk-reader.md
  - tasks/TASK-0248-bringup-rv-virt-v1_2a-host-virtio-net-dhcp-stub-loopback-deterministic.md
links:
  - Decision: docs/adr/0067-one-block-owner-backend-selected-by-fdt.md
  - Contract: docs/rfcs/RFC-0098-board-support-contract-fdt-truth-boot-chain.md (C4 coherence, C5, Phase 3)
  - The owner this renames: docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md, source/services/virtioblkd
  - The trait this implements: userspace/storage/src/lib.rs (`BlockDevice`); layout SSOT userspace/storage/src/layout.rs
  - Measurement: docs/board/measurements/2026-09-22-stock-system/README.md ("Storage", "DMA coherence")
  - Playbook: CLAUDE.md
---

## History

Seeded 2025-12-29 for a virtio-blk frontend + packagefs image builder; both shipped elsewhere
(virtio-blk v2 = TASK-0314, image builder = TASK-0260). Recut 2026-09-22 under the ledger-reuse
rule: the number now carries the board's block driver.

## Context (measured 2026-09-22)

The board's system disk is an eMMC (`AJTD4R`, 14.6 GiB, HS400 enhanced strobe, `boot0`/`boot1`
4 MiB, RPMB) behind `sdh@d4281000` (`spacemit,k1-x-sdhci`, 0x200 registers, IRQ 101, clocks
`sdh-io` + `sdh-core`, resets `sdh_axi` + `sdh2`, power domain 0); the microSD is
`sdh@d4280000` (IRQ 99, + `aib-clk`, card-detect GPIO); the SDIO WiFi function is
`sdh@d4280800` (IRQ 100). All three run ADMA in the stock system. The DMA master is not
cache-coherent (Zicbom/Svpbmt present, `swiotlb` in use). The eMMC has never been written
(first 256 bytes zero). Today `virtioblkd` is the single owner of the ONE GPT disk (ADR-0044)
over `storage-virtio-blk`; `BlockDevice` is the seam.

## Goal

`source/drivers/storage/sdhci` — a `BlockDevice` implementation over `nexus_hal::Bus` for the
SoC's SDHCI hosts (standard SDHCI register set + the SoC's vendor extras from the mainline
driver documentation): controller reset, clock via `nexus-soc`, card init (eMMC: CMD1/CMD2/CMD3,
ext-CSD, HS200/HS400 as the second step, HS at 52 MHz first), ADMA2 descriptor rings, IRQ
completion (`irq_bind`), bounded retries, `test_reject_*` for CRC/timeout/short-transfer
paths on a host mock. `virtioblkd` → **`blkd`** (ADR-0067): one service, backend chosen by the
FDT node init grants (`virtio,mmio` `device_id 2` or `spacemit,k1-x-sdhci`), same GPT parse,
same `blockproto`, same deny-by-default identity check; topology slot + policy class
(`device.mmio.blk` on QEMU, `device.mmio.mmc` on the board) + markers renamed with a gate
against the old name.

## Non-Goals

SD card hot-plug, SDIO (TASK-0248 uses this host driver as a client), NVMe, the eMMC's
hardware boot partitions/RPMB, write-back caching above the driver, performance tuning
(TASK-0317's bench gate).

## End state (binding)

- `blkd: backend=spacemit,k1-x-sdhci bus=8bit mode=hs400 sectors=…` on the board,
  `blkd: backend=virtio,mmio …` on QEMU; `packagefs: mounted` follows on both.
- `DmaBuffer` (nexus-hal) gains cache maintenance: `for_device(range)` / `for_cpu(range)` —
  Zicbom `cbo.clean`/`cbo.inval` on hardware, no-ops on QEMU (coherent), selected by the
  node's `dma-coherent` absence (RFC-0098 C4). ADMA descriptors live in `contiguous-DMA` VMOs
  from TASK-0286.
- A QEMU profile with `-device sdhci-pci -device sd-card,drive=…` (or `generic-sdhci`) runs the
  full block ladder over the new backend, so the driver is proven before the board: `ci-os-sdhci`.
- `virtioblkd` does not exist any more (crate, service name, markers, slots); `just check`
  fails on the old name.

## Packages

- **P0** — this recut; register map table from the mainline driver docs; the QEMU sdhci
  profile designed.
- **P1 Driver core** — host mock tests (command state machine, ADMA rings, error paths).
- **P2 `blkd`** — rename + backend selection + policy/slots/markers + old-name gate; QEMU
  virtio lanes green under the new name.
- **P3 QEMU sdhci profile** — `ci-os-sdhci` mounts the system volume over SDHCI.
- **P4 Board** — eMMC up on the serial console (needs B1.6's boot chain to run anything), GPT
  read, `packagefs: mounted`.

## Constraints / invariants

- One owner of the disk (ADR-0044/0067); no second block service.
- No `unwrap` on device data (ext-CSD, CID, responses are untrusted-shape input).
- Every DMA transfer is bracketed by the coherence hooks; a missing hook is a test failure on
  the mock (the mock tracks dirty ranges).

## Red flags / decision points

- **RED (R4, measured):** DMA is non-coherent on the board — the hooks are not optional and
  land in P1, before any board run.
- **YELLOW:** HS400 needs tuning + enhanced strobe; the ledger starts at HS (52 MHz) and adds
  HS200/HS400 as a measured step (throughput number in the log) rather than a first-boot risk.
- **GREEN:** the trait boundary is already clean; every consumer above `BlockDevice` is
  untouched by the rename.

## Definition of Done

Host tests (incl. `test_reject_*`); `ci-os-sdhci` + all existing lanes green under `blkd`;
board serial shows the two markers; old-name gate; docs (ADR-0067 Accepted, RFC-0098 Phase 3
✅, `docs/architecture` storage page, CHANGELOG).
