---
title: TASK-0246 Block driver on hardware: SDHCI/eMMC at `BlockDevice`, and `virtioblkd` becomes `blkd` — the one block owner with a backend chosen by the device it is granted
status: In Progress (P0 done 2026-09-24 — measured on the board, upstream and in QEMU; recut to the end state below; was recut 2026-09-22 as Block 1 B1.5 of the hardware fast track, originally "RISC-V Bring-up v1.1a: virtio-blk frontend core + packagefs image builder", whose subjects shipped as TASK-0314 and TASK-0260)
owner: @runtime @kernel-team
created: 2025-12-29
updated: 2026-09-24
depends-on:
  - tasks/TASK-0245-bringup-rv-virt-v1_0b-os-kernel-uart-plic-timer-uartd-selftests.md
  - tasks/TASK-0245B-board-support-v1c-soc-clock-reset-pinmux-power-from-fdt.md
  - tasks/TASK-0286-kernel-memory-accounting-v1-rss-pressure-snapshots.md
follow-up-tasks:
  - tasks/TASK-0246B-nxboot-sdhci-disk-reader.md
  - tasks/TASK-0248-bringup-rv-virt-v1_2a-host-virtio-net-dhcp-stub-loopback-deterministic.md
links:
  - Decision: docs/adr/0067-one-block-owner-backend-selected-by-fdt.md (amended 2026-09-24)
  - Contract: docs/rfcs/RFC-0098-board-support-contract-fdt-truth-boot-chain.md (C3 PCI source, C4 coherence + DMA reach, C5 — all amended 2026-09-24, Phase 3)
  - The owner this renames: docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md, source/services/virtioblkd
  - The trait this implements: userspace/storage/src/lib.rs (`BlockDevice`); layout SSOT userspace/storage/src/layout.rs
  - Measurements: docs/board/measurements/2026-09-24-emmc-sdhci/README.md (this P0), docs/board/measurements/2026-09-22-stock-system/README.md ("Storage", "DMA coherence")
  - Reused: `nexus_driverkit::DmaBuffer` over `nexus_abi::DmaVmo` (TASK-0286 P4b), `socd` (RFC-0106), `vmo_runs` (TASK-0286 P4a)
  - Playbook: CLAUDE.md
---

## History

Seeded 2025-12-29 for a virtio-blk frontend + packagefs image builder; both shipped elsewhere
(virtio-blk v2 = TASK-0314, image builder = TASK-0260). Recut 2026-09-22 under the ledger-reuse
rule: the number carries the board's block driver. Recut again 2026-09-24 at P0 after measuring
(board over `adb`, the mainline tree and driver, QEMU 11.1.1): the DMA reach of the storage
bus, the tuning-free way to the stock operating point, and the only QEMU stand-in changed the
plan; the text below is the end state.

## Context (measured 2026-09-24, `docs/board/measurements/2026-09-24-emmc-sdhci`)

- **The card:** eMMC 5.1 `AJTD4R`, 30 535 680 sectors (14.56 GiB), HS52/DDR52/HS200/HS400 at
  1.8 V with enhanced strobe, `boot0`/`boot1`/RPMB 4 MiB each, 64 MiB cache, command queue
  (depth 16) supported but off in the stock kernel, never written.
- **The host (`sdh@d4281000`, IRQ 101):** standard SDHCI plus K1 vendor registers from `0x108`
  (PHY enable, pad drive, MMC card mode, HS200/HS400 select, enhanced strobe, PHY DLL). The
  stock system runs **HS400 enhanced strobe, 8 bit, 1.8 V, 187.5 MHz** (375 MHz `io` clock ÷ 2)
  with **32-bit ADMA2** and zero errors. Quirks: capability clock base unusable (the base is the
  `io` clock rate), 64-bit DMA broken, 32-bit ADMA length field, timeout counted in SD clocks,
  no card detect, busy-wait on R1b. HS400ES needs a DLL lock, no delay-line tuning; HS200 and
  SDR104 need tuning.
- **DMA reach:** the SD hosts (and the DWC3) sit on the K1's `storage-bus`,
  `dma-ranges = [0, 2 GiB)` identity. The board's second bank (4 GiB at `0x1_0000_0000`) is out
  of their reach; other K1 buses translate addresses. Our `board.dts` has neither the buses nor
  the ranges, and our DMA buffers are in reach only because the pool fills bank 0 first.
- **Coherence:** the whole `soc` bus is `dma-noncoherent` (TASK-0286 P4b carries it into the
  device capability; `DmaBuffer` maintains).
- **QEMU:** no user-creatable sysbus SDHCI on `virt`; **`sdhci-pci`** (`1b36:0007`, class 0805,
  BAR0 256 B, unassigned) with the **`emmc`** card model is the stand-in. Default capabilities
  are an SDHCI v2 without an 8-bit bus (`capareg`/`sd-spec-version` are properties). `virt`'s
  `pci-host-ecam-generic`: ECAM `0x3000_0000` (256 MiB), 32-bit window `0x4000_0000` (1 GiB,
  identity), 64-bit window `0x4_0000_0000`, INTx → PLIC 32–35 through `interrupt-map`,
  `dma-coherent`. The OS has no PCI code.
- **Today (mapped 2026-09-24):** `virtioblkd` (551 lines, no `tests/`) is the single owner of
  the ONE GPT disk (ADR-0044) over `storage-virtio-blk` (1415 lines); `BlockDevice`
  (`userspace/storage`, synchronous, `&self` reads, 512-byte sectors assumed by `blockproto`,
  `RemoteBlockDevice`, nxboot and `nx`) is the seam and `blockproto` (7 partition selectors,
  `READ_VMO` bulk path) the wire; init grants `blk[0]` by lowest address after a probe of each
  `virtio,mmio` window. The virtio driver crate names the OWNER's watchdog slot
  (`slots::virtioblkd::WATCHDOG`) — a driver tied to its service. The name `virtioblkd` stands
  83 times in 33 source files outside its crate (topology ids/specs/slots/routes, init's core and
  blk planes, supervision, selftest probes), 7 times in `scripts/qemu-test.sh` +
  `qemu-launcher.sh`, once in `policies/base.toml`, plus docs (44) and ledgers (100). socd is
  provisioned AFTER the disk grant and the volume pass, only the harness has a route to it, and
  a driver cannot name its node (its capability carries the window, the line, the coherence —
  no path). nxboot finds the lowest `virtio,mmio` disk and runs `flow::run` over any
  `BlockDevice` — the SDHCI reader slots in without touching the flow.

## Goal

The board's eMMC is the system disk behind the same owner, the same GPT plane and the same
`blockproto` as QEMU's virtio disk — and the driver that makes it so is proven in a QEMU lane
before the board runs a single instruction of it.

## Non-Goals

SD-card hot-plug and SD as the system disk (the microSD host serves the stock system — the
desk fallback); SDIO (TASK-0248 is a client of this host driver); HS200/SDR104 and delay-line
tuning; the command queue (a measured step after the board runs, TASK-0317's bench gate);
`boot0`/`boot1`/RPMB; NVMe; the board's DesignWare PCIe host; write-back caching above the
driver; the board's image layout (the boot-ROM head partitions, the `swap` reservation for M7 —
TASK-0260, B1.6); the SDIO host, which shares its 4 KiB page with the SD host (measured —
TASK-0248's decision).

## End state (binding)

- **DMA reach travels with the device** (RFC-0098 C4): `board.dts` carries the K1 buses with
  their mainline `dma-ranges` for every node it has (`storage-bus`: the three SD hosts and the
  DWC3; `network-bus`: the two GMACs); `nexus_fdt` reads a device's ranges walking up from its
  node; `device_cap_create` takes a versioned descriptor (window, line, coherence, DMA ranges);
  a VMO made for a device is allocated within its reach; `vmo_runs` names the device, answers
  its BUS addresses, refuses a run outside its reach and requires that device's capability.
- **`source/drivers/storage/sdhci`** — one crate, two layers: the standard core (reset, clock
  divider, command engine with R1/R1b/R2/R3, eMMC init CMD0 → CMD1 → CMD2 → CMD3 → CMD9 →
  CMD7 → CMD8 → CMD6 to HS52 8-bit, ADMA2 with 32-bit descriptors built from `DmaRun`s, IRQ
  completion) and the `K1` layer (vendor registers, HS400 enhanced strobe with the PHY DLL).
  Every transfer goes through `DmaBuffer` (`for_device`/`for_cpu`); the only polls are the
  controller states that raise no interrupt (internal clock stable, reset done, DLL lock), each
  bounded and named. A `BlockDevice` implementation on top.
- **PCI ECAM is a device source** (RFC-0098 C3): one pure planner (bus scan, BAR sizing and
  lowest-first assignment from the host node's ranges, INTx through `interrupt-map`) for init
  and nxboot; `init: devices from pci ok (…)`.
- **`blkd`** replaces `virtioblkd` (ADR-0067): one service, the backend chosen by the device
  init grants (virtio `device_id 2`, `spacemit,k1-sdhci`, PCI class 0805), the device being the
  one nxboot booted from (`/chosen/nexus,boot-disk`); same GPT parse, `blockproto`,
  deny-by-default identity check; `blkd: backend=… ready` markers; `virtioblkd` exists nowhere
  and `just check` fails on the name.
- **`ci-os-sdhci`** in `test-all`: `sdhci-pci` (8-bit v3 capabilities) + `emmc` carrying the
  system image, no virtio disk; nxboot reads it (TASK-0246B), the full ladder mounts the system
  volume over the SDHCI backend.
- **Board:** `blkd: backend=spacemit,k1-sdhci bus=8 mode=hs400es sectors=30535680 …` and
  `packagefs: mounted` on the serial console, with the throughput in the log.

## Packages

- **P0 Paper + measurement — done 2026-09-24.** Measured the card (EXT_CSD, identity,
  geometry), the host's operating point and DMA mode, the vendor tree's node, the mainline
  binding, bus layout and driver (register facts, quirks, sequence), and QEMU's SD devices and
  PCI host (`docs/board/measurements/2026-09-24-emmc-sdhci`). Found: the storage bus's DMA
  reach (correct by luck today), the tuning-free route HS52 → HS400ES, 32-bit ADMA2, and that
  QEMU's only SD host is behind PCI. RFC-0098 amended (C3 PCI source, C4 DMA reach, C5 names +
  boot disk + operating point), ADR-0067 amended. This recut.
- **P1 DMA reach in the device capability** — the tree (`board.dts` buses + `dma-ranges`, golden
  regenerated), `nexus_fdt::Node::dma_ranges` (host goldens: board SD hosts `[0, 2 GiB)`, GMACs
  translated, virt identity) and `reg` translation through every level (today one level —
  right for `storage-bus` under `soc` only because both map the identity), the versioned device descriptor, the kernel's per-device reach
  (allocation within reach; `vmo_runs(vmo, device, …)` in bus addresses, reach-checked, that
  device only), `DmaVmo::for_device`; `test_reject_*` for a run outside reach, the wrong device
  and an unknown descriptor version; the P4a/P4b proofs move onto the device-scoped form.
- **P2 SDHCI core, host-first** — the crate over `nexus_hal::Bus` against a behavioural
  SDHCI + eMMC model in the tests (commands, responses, ADMA2 fetch from model memory, IRQ
  status, and cache-state tracking so a missing `for_cpu`/`for_device` is a test failure); the
  eMMC init to HS52 8-bit; the K1 layer's register sequence and HS400ES step; `test_reject_*`:
  command timeout, CRC error, data timeout, ADMA error, short transfer, R1 error bits, unexpected
  card state, EXT_CSD with a zero or out-of-range sector count.
- **P3 PCI ECAM source** — the pure planner (host tests over the virt golden and synthetic config
  spaces: BAR sizing, 32/64-bit assignment, swizzled INTx), init glue and the
  `init: devices from pci ok (…)` marker; the SDHCI function granted by class.
- **P4 `blkd`** — rename (crate, service, topology ids/specs/slots/routes, init planes,
  supervision, policy, markers, scripts, docs) with the old-name gate; the driver crates take
  their slots from the owner (no service's slot name inside a driver); backend selection by the
  granted device; `/chosen/nexus,boot-disk` written by nxboot and honoured by init; the
  partition gate host-tested with `test_reject_*` in the new `tests/` (the service had none);
  socd joins the core plane BEFORE the disk grant and `blkd` asks it to bring up its node —
  named by the window its capability carries (socd resolves the node by `reg`), `NotNeeded` on
  QEMU; the stale texts the map found (`remote_blk` "2 s deadline", an unemitted watchdog marker,
  `reply_inbox`, the launcher's `/data` device) corrected; every existing lane green under the
  new name.
- **TASK-0246B nxboot readers** — between P4 and P5 (the QEMU lane boots through it).
- **P5 `ci-os-sdhci`** — the launcher profile and the lane in `test-all`; the system volume
  mounted over the SDHCI backend; the proof of the standard core.
- **P6 Board** — after B1.6 boots our chain: eMMC at HS52, then HS400ES, GPT read,
  `packagefs: mounted`; the markers join TASK-0327B's ladder.

## Constraints / invariants

- One owner of the disk (ADR-0044/0067); no second block service; the GPT plane and
  `blockproto` do not change.
- No `unwrap` on device data (responses, CID/CSD, EXT_CSD are untrusted-shape input); every
  field used is range-checked.
- Every DMA transfer is bracketed by `DmaBuffer`; every DMA address the host sees came from
  `vmo_runs` for THIS device (in reach, in bus addresses).
- Deterministic by construction: no tuning; the only waits without interrupts are bounded and
  named; nxboot and init share one PCI assignment; the disk is the one the boot came from.

## Red flags / decision points

- **RED (measured): DMA reach.** Without P1 the first full bank 0 turns SD transfers into
  silent corruption; P1 lands before any SDHCI transfer runs anywhere.
- **YELLOW: HS400ES on the board is a board-only proof** — QEMU has no vendor registers and no
  HS400; the K1 layer is proven by the host model first and by the board last (HS52 is the
  fallback the driver reports, not hides).
- **YELLOW: QEMU `emmc` fidelity** — the model's CMD6/bus-width behaviour at 8 bit is measured
  in P5; if it cannot do 8 bit the lane runs 4 bit and says so in its marker.
- **YELLOW (measured): socd's place in the boot order.** The disk owner must have its node
  brought up before it touches the controller; socd moves into the core plane in P4 (it needs
  only policyd, which is already there) — on the board the SPL left the eMMC clocked, so the
  first plan writes nothing (0245B's tests), but "it happens to be on" is not the contract.
- **GREEN:** the trait boundary is clean; `blockproto` consumers are untouched by the rename.

## Definition of Done

Host tests incl. `test_reject_*` (reach, driver, planner); `ci-os-sdhci` and every existing
lane green under `blkd`; the board's serial shows the backend and `packagefs: mounted`; the
old-name gate; docs (ADR-0067 Accepted, RFC-0098 Phase 3 ✅, `docs/architecture` storage page,
CHANGELOG).
