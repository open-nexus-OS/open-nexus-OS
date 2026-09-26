---
title: TASK-0246B nxboot reads the boot medium through the same SDHCI core as `blkd` — on the board, and behind QEMU's PCI host — and names it in `/chosen`
status: In Progress (P2 done 2026-09-26 — the board path: the SD hosts' measured roles in the board tree, a `no-mmc` host no disk kind, `nexus-soc` bring-up and the `io` clock in the loader, host-proven; the board proves it in TASK-0246 P6 / TASK-0260B; P1 done 2026-09-25 — the PIO write path, nxboot's disk rule with the SDHCI reader, the PCI plan in nxboot, the record; QEMU boots from `sdhci-pci` end to end, and since TASK-0246 P5 (2026-09-26) the `ci-os-sdhci` lane in `test-all` boots through the reader; P2 next — the board path; recut 2026-09-25 to the end state below after TASK-0246 P2–P4c; seeded 2026-09-22 at Block 1 P0 as the B part of TASK-0246, recut 2026-09-24 at TASK-0246 P0)
owner: @runtime @reliability
created: 2026-09-22
updated: 2026-09-26
depends-on:
  - tasks/TASK-0246-bringup-rv-virt-v1_1a-host-virtio-blk-image-builder-deterministic.md (P2 SDHCI core, P3 PCI planner, P4b boot-disk record)
follow-up-tasks:
  - tasks/TASK-0260B-nxboot-as-fit-payload-chosen-node.md
links:
  - Decisions: docs/adr/0066-boot-chain-on-hardware-nxboot-as-fit-payload.md, docs/adr/0067-one-block-owner-backend-selected-by-fdt.md (amended 2026-09-24)
  - Contract: docs/rfcs/RFC-0098-board-support-contract-fdt-truth-boot-chain.md (C3 PCI source, C5 boot disk and its record), docs/rfcs/RFC-0106-soc-glue-one-owner-clocks-resets-power-pinmux.md (the bring-up library)
  - nxboot today: source/boot/nxboot/src/virtio.rs (`VirtioDisk::probe`), src/flow.rs (BSB → slot → GPT → NXBD, generic over `storage::BlockDevice`), src/platform.rs (`/chosen`, `boot_disk_record`)
  - The core it reuses: source/drivers/storage/sdhci (`Card::read_pio`), its machine `storage-sdhci-model`, `storage::boot_disk` (the record), `nexus-pci` (the planner init runs)
  - Measurement: docs/board/measurements/2026-09-24-emmc-sdhci/README.md
  - Playbook: CLAUDE.md
---

## Context (measured 2026-09-24, re-read 2026-09-25 after TASK-0246 P2–P4c)

nxboot's disk reader is `virtio.rs`, a virtio-blk MMIO reader; `flow::run` is generic over
`storage::BlockDevice` and **writes**: the A/B trial decrements the BSB before the load
(`write_alternate`, RFC-0089 §7) — so a reader for nxboot must write single sectors too. The
SDHCI core (TASK-0246 P2) reads by PIO (`Card::read_pio`) but writes only by ADMA, and its
behavioural machine refuses PIO writes ("PIO writes are not driven"). nxboot has no clock
function (the core's `Platform` needs `now_us` and `delay_us`); the tree carries the timebase.
On QEMU the SDHCI stand-in is `sdhci-pci` behind the ECAM host with an UNASSIGNED BAR — nothing
before nxboot enumerates PCI; the planner init runs (TASK-0246 P3) places it deterministically,
so both stages see one assignment. No SPL runs before nxboot on QEMU; on the board the vendor SPL
read the FIT from the eMMC, so the host is clocked there — but its controller, card and clock
state are the SPL's, not a contract. The board carries three SD hosts (the SD slot, the SDIO
WiFi function, the eMMC) and may hold a stock SD card; the K1's capability register names no base
clock (the `io` clock's rate is the base clock). The OS reads the record nxboot writes
(`/chosen/nexus,boot-disk`, TASK-0246 P4b) and grants exactly that device.

## Goal

nxboot reads — and writes the BSB of — the boot medium through the SDHCI core as well as the
virtio reader, finds it by a fixed rule, and names it in `/chosen/nexus,boot-disk`.

## End state (binding)

- **The core writes by PIO** (`Card::write_pio`: CMD23 + CMD25, the data port per block, the card
  back in transfer state when it returns), host-proven against `storage-sdhci-model`, which
  drives PIO writes and refuses them on the same faults it refuses reads.
- **The `disk` seam** — `nxboot::disk` (the rule, the SDHCI reader; host-tested) and
  `src/probe.rs` (the candidates on the target): candidates in a fixed
  order — virtio block transports (lowest address first), SD hosts in the tree that may hold an
  eMMC (`spacemit,k1-sdhci` without `no-mmc`, lowest address first), SD hosts behind an ECAM host
  (the planner's order) — and **the first whose BSB is valid is the boot disk**; a candidate that
  does not answer, or answers without our BSB, is named and skipped. The SDHCI reader is a
  `BlockDevice` over `Card` in PIO mode (no DMA, no interrupts — nxboot has neither), initialising
  the card itself at the rule's operating point (HS52 on the widest bus; no HS400 in the loader)
  and relying on no predecessor's controller state, so QEMU and the board take one path.
- **nxboot's clock**: `rdtime` over the tree's timebase, the core's `Platform` (bounded busy waits,
  no interrupt).
- **nxboot plans PCI** with `nexus-pci` over physical ECAM windows (the assignment init repeats);
  bus mastering stays off (PIO).
- **The record**: the node path, or `<ECAM host path>/mmc@<dev>,<func>`
  (`storage::boot_disk::record_for_pci_sd_host`), for the disk the volume was read from.
- **The board** (P2): the board tree marks each SD host's role the standard way, as the live
  tree does (measured) — so the eMMC host is the board's only SD candidate — and carries the
  eMMC host's HS400ES capability. nxboot brings the candidate's node up with `nexus-soc` (the
  library socd runs — power domain, resets, clocks, every write read back; the eMMC binds no
  pads) and reads its `io` clock's rate from the registers for the K1 layer's base clock. The
  bring-up and the rate live in `nexus-soc` once, for socd and the loader. Host-proven against
  the board's golden tree, the measured register state and the K1 machine; the board proves it
  in TASK-0246 P6 / TASK-0260B.

## Non-Goals

ADMA, the command queue, HS400 in the loader (the OS drives the operating point), SD-card boot
(the boot ROM tries SD first; a stock SD card in the slot boots the stock system — the desk
fallback by design), the SD-card protocol (the core drives eMMC only).

## Packages

- **P1 — the SDHCI reader (QEMU).** `Card::write_pio` + the model's PIO writes; the `disk` seam
  and its candidate rule; nxboot's clock; the PCI plan in nxboot; the record for a PCI SD host.
  Host: the core's PIO write matrix against the machine; the reader over the machine (a flow
  run: BSB decrement written, image streamed); the candidate rule (virtio first, the first valid
  BSB wins, a silent or foreign disk skipped). QEMU: a manual boot with `sdhci-pci` + `emmc`
  holding the system image and no virtio disk — nxboot reads and writes through the reader,
  init grants the recorded function, `blkd` runs the SDHCI backend. TASK-0246 P5 turns it into
  the `ci-os-sdhci` lane.
- **P1 done 2026-09-25.** The core writes by PIO (`Card::write_pio`: CMD23 + CMD25, the data
  port per block, the status read after programming) and the machine drives PIO writes. Writing
  the tests found a latent core bug: a PIO transfer the controller ended early (blocks left)
  waited for a buffer that never comes — a data timeout on hardware, a hang in the machine,
  whose time froze while any interrupt was pending; the PIO loops now take the early end as a
  named short transfer, and the machine lets time pass under a pending interrupt, so a driver
  waiting for the wrong bit meets its deadline instead of hanging a test. nxboot's library half
  holds the rule (`disk::pick`: the first candidate that opens and carries a valid BSB, read by
  `flow::read_boot_state` — the read every decision starts with) and the reader
  (`disk::sdhci::SdhciDisk`, PIO at HS52); the target half (`src/probe.rs`) lists the candidates
  (virtio block transports, `spacemit,k1-sdhci` hosts without `no-mmc`, SD hosts behind each ECAM
  host placed by `nexus-pci`), runs the reader over physical registers and `rdtime`
  (`arch::time_ticks`), names each skipped candidate and records the chosen one. Proof:
  host — `storage-sdhci` 16 + 38 (7 new PIO tests; three
  mutations of the write path each fail them), nxboot `loader_flow` 12 (4 new: the rule, a clean
  boot and the trial ladder over the SDHCI reader, a silent host skipped before a card
  re-initialised from power-up); `just check` green; `build-os-workspace` 0 warnings;
  `ci-os-smp1` green; `just test-all` green (EXIT=0, 11 QEMU lanes; nxboot records a virtio disk
  in all 21 of its boots and skips no candidate; `blkd: backend ok` in all 19 boots that reach
  init). Manual QEMU boots from `sdhci-pci` + `emmc` alone (the system image padded to 4 GiB):
  nxboot `disk=/soc/pci@30000000/mmc@1,0`, init `master=on`, `blkd: backend ok (kind=sdhci-pci …
  mode=hs52 bus=4 sectors=8388608)`, the system volume (23 bundles), packagefs, statefs and nxfs
  mounted; a fresh image shows no FAIL beyond the known dsoftbus pair (an image reused from
  earlier lanes — BSB seq 6 — failed `ota delta stage`; the fresh one did not).
- **P2 — the board path — done 2026-09-26.** Measured 2026-09-26 over adb (the live vendor tree,
  `docs/board/measurements/2026-09-24-emmc-sdhci/live-tree-sdh.txt`): the microSD host carries
  `no-mmc` + `no-sdio`, the SDIO host `no-mmc` + `no-sd` + `non-removable`, the eMMC host
  `no-sd` + `no-sdio` + `non-removable` + `mmc-hs400-1_8v` + `mmc-hs400-enhanced-strobe`. Our
  board tree carried none of them: the rule would have tried the SD slot and the SDIO function
  as eMMCs first (an op-cond bound each), and the eMMC node allowed no HS400ES. The board tree
  takes the measured roles and the eMMC's capability (the golden rebuilt). `nexus-soc` owns a
  node's bring-up (`bring_up`: plan, then execute; an empty plan is `NotNeeded`) and a named
  clock's rate (`clock_rate`), both moved out of socd, which now calls them. nxboot's library
  half gets `disk::node`: the hosts the tree lets hold an eMMC, and a host's configuration once
  its glue is up and its `io` clock read. The target half runs it over physical windows, for the
  one candidate it is about to open. RFC-0106 records the loader clause (before any service
  exists, the loader runs the same library; socd stays the one writer while the OS runs).
  Host: the board offers only its eMMC host; from the measured state the loader writes nothing
  and reads 375 MHz, from cold it writes exactly the documented bits; a whole boot decision over
  the K1 machine at the derived clock; glue that does not read back and a host without an `io`
  clock are skipped with their reason. The board proves it in TASK-0246 P6 / TASK-0260B.
  A K1 host marked `no-mmc` is no disk kind (`storage::boot_disk::Kind::of_node`), so no stage —
  the loader's rule, init's grant, the owner's selection — names it. A glue fault prints the
  register and the value it read (`nxboot: bring-up <node> FAIL (reg=… val=…)`, socd's words)
  next to the skip line. Proof: host — `nexus-soc` 16 (4 new: the node operations — not needed
  on virt, up without a write from the measured state and with four from cold, refused before
  any bus access, a fault with its register; `io` reads 375 MHz on the eMMC and 204.8 MHz on the
  microSD host, the live tree's `spacemit,sdh-freq`); `storage` (a record naming a `no-mmc` host
  refused, by path and by window; the board's eMMC allows HS400ES); nxboot `loader_flow` 19 (7
  new: the tree's hosts lowest address first; the board offers only its eMMC host; from the
  measured state no write and 375 MHz, from cold the four documented writes and 409.6 MHz; a
  whole boot decision over the K1 machine at the derived clock; glue that does not read back,
  glue the tables do not cover, a host without an `io` clock and a bus no host has refused by
  name); five mutations (the `no-mmc` guard, seen by both suites; the bring-up; the clock name;
  the address order) each fail their test; socd's contract 7 unchanged. `just check` green;
  `build-os-workspace` 0 warnings; `just test-all` green (EXIT=0, 12 QEMU lanes). QEMU behaves
  as before: nxboot records a virtio disk in 20 of its 21 boots and the SD host in the SDHCI
  one, skips nothing and never reaches the board path (no K1 host in QEMU's tree); `blkd:
  backend ok` in all 19 boots that reach init; socd answers `no soc glue in this tree` in all
  19; smp1 shows 430 and the SDHCI lane 429 ok lines, as before the package, with the same two
  allowlisted dsoftbus FAIL lines.

## Red flags / decision points

- **YELLOW: a candidate that never answers costs its bounded init** (an empty host: the op-cond
  bound, one second). On the board the tree marks the SD slot and the SDIO host `no-mmc`
  (measured, P2), so the eMMC host is the only SD candidate; QEMU lanes attach one disk.
- **YELLOW: QEMU's `emmc` is byte-addressed at or below 2 GiB** (P0) — the core drives
  sector-addressed cards only, so the lane's image is larger than 2 GiB (sparse).
- **GREEN:** the flow, the BSB/GPT/NXBD decisions and the markers are untouched; the reader is
  one more `BlockDevice`.

## Definition of Done

Host: the PIO write matrix, the reader under a flow run, the candidate rule ✅ (P1); the board
path — the tree's roles, the glue and the `io` clock — against the golden tree, the measured
register state and the K1 machine ✅ (P2, 2026-09-26); QEMU:
`ci-os-sdhci` boots through nxboot's SDHCI reader and `init` grants the device named in
`/chosen` (TASK-0246 P5) ✅ 2026-09-26; the board: `nxboot: platform=<compatible> slot=<a|b>`
(TASK-0260B) with the eMMC as the recorded disk; ADR-0066/0067 consequences recorded.
