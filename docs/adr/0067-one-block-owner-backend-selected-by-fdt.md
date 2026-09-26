# ADR-0067: ONE block owner (`blkd`) whose backend the FDT selects — virtio-blk on QEMU, SDHCI on the board; the GPT plane above it never learns the difference

- Status: Proposed
- Date: 2026-09-22 (amended 2026-09-24)
- Links:
  - Tasks: `tasks/TASK-0246-*` (SDHCI driver + `blkd`), `tasks/TASK-0246B-*` (nxboot reader)
  - RFCs: `docs/rfcs/RFC-0098-board-support-contract-fdt-truth-boot-chain.md` (C5),
    `docs/rfcs/RFC-0017-device-mmio-access-model-v1.md` (grants by class)
  - Related ADRs: `docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md` (the single
    owner and the partition plane this keeps), `docs/adr/0066-*` (nxboot shares the readers)
  - Measurement: three `spacemit,k1-x-sdhci` hosts, eMMC HS400 with ADMA, non-coherent DMA
    (`docs/board/measurements/2026-09-22-stock-system/README.md`)

## Context

ADR-0044 made `virtioblkd` the single owner of the ONE GPT disk: it parses the table once and
serves partition-scoped `blockproto` to statefsd, nxfsd, updated and bootctld, deny-by-default on
the kernel-attributed sender. Everything above the `userspace/storage::BlockDevice` trait —
GPT, the layout SSOT, `blockproto`, the consumers — is device-agnostic already. The board has
no virtio: its system disk is an eMMC behind an SDHCI host (ADMA, HS400), with a microSD and an
SDIO WiFi function on two more identical hosts.

The fork: a second service for the board's disk (two owners, two names, two policy classes),
or the same owner with a second backend.

## Decision

- **`virtioblkd` becomes `blkd`, the one block owner on every platform.** It is granted ONE
  device node by init (RFC-0017 class `device.mmio.blk` on QEMU, `device.mmio.mmc` on the
  board) and selects its `BlockDevice` backend by that node's compatible: `virtio,mmio` with
  virtio `device_id 2`, or `spacemit,k1-sdhci` (corrected 2026-09-24, see the amendment). One binary, one policy identity, one GPT
  parse, the same `blockproto`. (Renamed 2026-09-25, TASK-0246 P4a: the crate, service id,
  slot table, policy row and markers; the partition gate became a host-proven module; the
  virtio driver takes its watchdog slots from its owner; `scripts/check-retired-names.sh` in
  `just check` keeps the old spellings out of the living tree. Implemented 2026-09-25, P4b:
  the backend is chosen from the loader's `/chosen/nexus,boot-disk` record, which `blkd` reads
  from its read-only tree and checks against the window it was granted; a PCI SD host is named
  `<host>/mmc@<dev>,<func>`; the classes are `device.mmio.blk` and `device.mmio.mmc`, and
  `blkd` is their only holder. Implemented 2026-09-25, P4c: socd runs before the disk is
  granted, and `blkd` has it bring the disk's node up — and name the K1's `io` clock rate —
  before it touches the controller.)
- The SDHCI backend lives in `source/drivers/storage/sdhci` over `nexus_hal::Bus`, ADMA2 from
  the start (the stock system proves the host does it), with `DmaBuffer` cache maintenance
  because the master is not coherent (RFC-0098 C4). It serves eMMC and SD; SDIO (the WiFi
  function) is a client of the same host driver later (target picture N), not of `blkd`.
- nxboot carries the same two readers (read-only, one block at a time) so the first-stage
  loader and the OS agree on the medium (ADR-0066).
- The layout SSOT (`userspace/storage/src/layout.rs`) is the one place that knows the
  board's head partitions (`bootinfo`, `fsbl`, `env`, `opensbi`, the FIT slot) in addition to
  the volumes; consumers keep addressing partitions by name.
- Out of scope: hot-plug of the microSD, multiple simultaneous disks, NVMe (a third backend
  when the PCIe path exists), the eMMC's `boot0`/`boot1` hardware partitions and RPMB.

## Amendment 2026-09-24 (TASK-0246 P0, measured)

Measured on the board, upstream and in QEMU (`docs/board/measurements/2026-09-24-emmc-sdhci`):

- **Names.** The board tree's compatible is the mainline `spacemit,k1-sdhci`; the stock
  system's `spacemit,k1-x-sdhci` belongs to the vendor tree we do not boot.
- **Reach.** The SD hosts sit on the K1's `storage-bus`, whose `dma-ranges` covers only the
  first 2 GiB. The device capability carries that reach and the kernel allocates the driver's
  DMA memory inside it (RFC-0098 C4) — the ADMA table and the data buffers alike.
- **The operating point without tuning.** HS52 (8 bit, SDR) first, then HS400 with enhanced
  strobe — the stock system's own mode: the PHY DLL locks, no delay-line tuning runs. HS200 is
  not a step. ADMA2 with 32-bit descriptors (the host's `64-bit DMA broken` quirk and the bus's
  reach agree).
- **The QEMU backend is real.** `sdhci-pci` + the `emmc` model prove the standard core (reset,
  clock, commands, ADMA2, IRQ completion, the eMMC init to HS52 8-bit) in a lane, through the
  PCI device source (RFC-0098 C3); the board proves the K1 layer (the lane: implemented
  2026-09-26, TASK-0246 P5 — `just ci-os-sdhci` in `test-all`; the loader reads the eMMC by
  PIO, `blkd` serves it by ADMA2 on its interrupt at HS52 on 8 bits, every store mounts over
  it). One `sdhci` crate, two layers: `Standard` and `K1` (implemented 2026-09-25, TASK-0246
  P2: `source/drivers/storage/sdhci`, eMMC only — the SD-card protocol is outside TASK-0246
  and arrives when an SD medium is needed).
- **Which disk.** nxboot names the medium it booted from in `/chosen/nexus,boot-disk`; init
  grants exactly that device to `blkd` (the board has three SD hosts and a stock SD card).
- **nxboot initialises the card itself** with the same core in PIO mode — it relies on no
  predecessor's controller state, so QEMU (no SPL) and the board (SPL before it) take one path.

## Consequences

- **Positive**: the storage ladder, OTA, statefs, nxfs and bootctld run unchanged on the
  board; the QEMU lanes keep proving the owner while the board proves the backend; one
  service to reason about for the disk lock.
- **Negative / accepted cost**: a rename (`virtioblkd` → `blkd`) touches the topology slot
  names, policy table and markers (`virtioblkd: …` → `blkd: …`) in one package with a gate
  against the old name; the SDHCI driver is the first non-coherent DMA master and pays for the
  `DmaBuffer` cache hooks that every later driver reuses.
- **Follow-ups**: TASK-0246 (driver + rename + QEMU `sdhci` profile), TASK-0246B (nxboot),
  TASK-0248 (SDIO function on the same host driver).

## Alternatives considered

- **A separate `emmcd` next to `virtioblkd`** — rejected: two owners of "the disk", two policy
  classes, two GPT parsers, and every consumer would need to know which one exists.
- **The block driver in the kernel** — rejected: drivers are userspace services in this OS
  (CLAUDE.md, ADR-0039); the kernel grants MMIO and IRQs, it does not talk to eMMC.
- **PIO-only SDHCI first** — rejected as an interim; ADMA is what the host is built for and the
  cache-maintenance seam is needed by every DMA driver after this one.
