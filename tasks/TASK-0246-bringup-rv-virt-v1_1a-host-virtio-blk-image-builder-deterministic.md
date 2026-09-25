---
title: TASK-0246 Block driver on hardware: SDHCI/eMMC at `BlockDevice`, and `virtioblkd` becomes `blkd` — the one block owner with a backend chosen by the device it is granted
status: In Progress (P4a done 2026-09-25 — `virtioblkd` became `blkd`, its partition gate host-proven, the old name gated; P4b next — the boot disk from `/chosen`, the backend by the granted device; P3 done 2026-09-25 — the PCI ECAM device source, `init: devices from pci ok`; P2 done 2026-09-25 — the SDHCI core, host-proven against a behavioural controller + eMMC model; P1 done 2026-09-24 — a device's DMA reach is kernel truth, `vmo_runs` answers in its bus addresses; P0 done 2026-09-24 — measured on the board, upstream and in QEMU; recut to the end state below; was recut 2026-09-22 as Block 1 B1.5 of the hardware fast track, originally "RISC-V Bring-up v1.1a: virtio-blk frontend core + packagefs image builder", whose subjects shipped as TASK-0314 and TASK-0260)
owner: @runtime @kernel-team
created: 2025-12-29
updated: 2026-09-25
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
  - The owner (renamed in P4a): docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md, source/services/blkd
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
- **P1 DMA reach in the device capability — ✅ done 2026-09-24.** The tree: `board.dts` gains
  `storage-bus` (sdhci0/1/2, the DWC3, an EHCI host, the UDC; `dma-ranges` = identity over
  `[0, 2 GiB)`), `network-bus` (the GMACs) and `multimedia-bus` (display controller, GPU), both
  with that identity plus bus `0x8000_0000` → CPU `0x1_0000_0000`; golden regenerated.
  `nexus_fdt::Node::dma_reach` composes the `dma-ranges` of every level above a node (absent =
  the walk ends, empty = identity, > 4 windows = malformed) and `Node::reg` translates through
  every level; goldens: the board's SD hosts `[0, 2 GiB)`, the GMACs and the display side
  translated, virt constrains no master, a nested fixture for composition and translation.
  The descriptor: `nexus_abi::DeviceDesc` v1 (128 bytes, `repr(C)`, layout pinned by const
  asserts on both sides); the kernel's `mm/dma_reach.rs` (pure, host) decodes it deny-by-default
  and keeps one immutable record per register window, `mm/devices.rs` is the live table; the
  capability names its record (`DeviceMmio { .., dev }`). Allocation within reach:
  `frames::alloc_within` / `alloc_at_most_within` (first-fit per window, a straddling free
  block carved; unit tests + a proptest that in-window blocks stay inside, never overlap and all
  come back), `alloc_below` deleted; `vmo_create(slot, len, flags, device)` — a contiguous
  object without a device is refused, which replaces the `dma-contiguous` path gate (deleted).
  `vmo_runs(vmo, device, offset, len, out, max)` answers in bus addresses, reach-checked, only
  for the holder of THAT device; `DmaRun::pa` → `bus`; `DmaVmo::{contiguous, anonymous}(device,
  len)`. Every driver makes its DMA memory for its device (virtio-blk, -net, -rng, -input; gpud's
  queues and the backings the GPU reads, its CPU-only scratch stays unbound). `test_reject_*`:
  malformed and bad-window descriptors, a second description of one window, a full table and
  unknown ids, a run out of reach, runs outside the device's reach, windows without memory. The
  P4a/P4b proofs moved onto the device-scoped form: `KSELFTEST: vmo runs ok (runs=2 deny=4)`
  (no slot, a VMO as the device, a read-only alias, past the end) and the new
  `KSELFTEST: vmo reach ok (window=0x90000000+0x4000000 bus=0x4000000000 runs=1 deny=2)` (a
  synthetic device whose one translated window is the top of the first bank: anonymous and
  contiguous objects placed inside, runs translated whole and at an inner offset, an object made
  for another device outside refused, a contiguous object without a device refused; the synthetic
  records are retired before init). Measured on smp1: the memory record is byte-identical to P5's
  (54 objects, 70 033 408 bytes, 272 KiB DMA, `exhausted=0`); gpud attaches windowd's 49 MB
  framebuffer in 5 runs through the GPU's reach. Found on the way: a one-page object for no
  device can land in a carve remainder anywhere (the buddy takes the smallest order first), so
  the selftest's "outside" object is made for a second device by construction. Proof: kernel
  host tests 107/107, nexus-fdt goldens 17/17 and every touched crate's host tests green;
  `just check` green (the size ratchet shrank virtio-rng 656 → 649 and virtio-input 873 → 870
  lines); smp1 twice with byte-identical DMA markers, backing runs and memory record; visible
  green (pixel proof: the greeter over the GL path); `just test-all` green (EXIT=0, 11 QEMU
  lanes: smp1, visible ×2 pixel proofs, input-flood, reset, seven OTA profiles; both markers in
  all 20 boots with identical numbers).
- **P2 SDHCI core, host-first — ✅ done 2026-09-25.** `source/drivers/storage/sdhci`
  (`storage-sdhci`: pure, `no_std`, `forbid(unsafe_code)`, eleven modules of at most 380 lines).
  The standard core speaks `nexus_hal::Bus` in aligned 32-bit words only. It covers reset,
  power at the highest voltage the controller names, the divider for spec 2 and 3 (with the
  4.10 PLL), width, timing with the card clock stopped, and 1.8 V. The command engine does
  R1/R1b/R2/R3 with interrupt completion and a named deadline per stage, PIO, and ADMA2 with
  32-bit descriptors built from `DmaRun`s (END on the last transfer descriptor). It checks
  the residual block count and resets the lines after an error. The K1 layer (`k1.rs`) sets,
  after reset, PHY + PLL lock, drive 4 + RX bias, MMC card mode and the pad clock. It sets the
  transmit clock per timing, the HS400 mode bit, and the enhanced strobe with the DLL
  (pre-delay, range and regulator at 1, register 1 = 0x92) locked within 100 µs. The eMMC
  (`card.rs`): JEDEC identification with sector addressing required, the EXT_CSD by PIO at
  legacy speed, the volatile cache kept off (every write is durable when it completes). The
  mode follows a rule, never a trial: HS400 enhanced strobe in JEDEC order (HS timing, 8-bit
  DDR with strobe, HS400, then the host's strobe and DLL at the operating clock), else HS52 on
  the widest bus, else legacy. Every switch is followed by the card's status, and the final
  bus is verified by reading the EXT_CSD again. `init_best` falls back from a failed HS400ES
  to HS52 from a fresh reset and returns the reason. Transfers are CMD23 + CMD18/CMD25; after
  a data error the card is stopped (CMD12) and must be back in transfer before anything else
  runs. `Disk` moves sectors through a descriptor table and a bounce buffer, both
  `DmaBuffer`s, in chunks; `Card::read_pio` reads without DMA or interrupt (for 0246B). The
  `BlockDevice` adapters live with their consumers (blkd in P4, nxboot in 0246B): the
  `storage` crate depends on the virtio driver, and the core depends on no service. Every
  wait bound is public (`timeouts`). `nexus_driverkit::CacheOps::flush` now takes the CPU
  view `&mut`: after an invalidating flush the CPU reads what memory holds, and that exact
  semantics is what lets the host model be exact. Proof: 16 unit tests (the divider at the
  measured operating points 399.8 kHz, 23.4, 46.9 and 187.5 MHz; descriptors; protocol
  decoding; the board's measured EXT_CSD) and 31 tests against a behavioural controller +
  eMMC model with an exact non-coherent cache (`tests/sdhci`). They cover the command
  sequences and the K1 vendor-register sequence as goldens, HS52 on 4 and 8 bits, HS400ES at
  187.5 MHz, the DLL fallback, determinism to the microsecond, ADMA2 through four scattered
  runs, PIO, and the ledger's `test_reject_*` list plus byte addressing, refused switches, a
  corrupting bus, ranges, unreachable DMA memory and bad configurations. Two mutations — no
  cache maintenance, and a read handed to the device as a write — each fail 8 tests. The
  strict model found a real bug before any hardware ran: a local 512-byte constant shadowed
  the block register's name, so the block word went to offset 0x200. Gates: `just check`
  green, the crate lints clean and cross-builds for the OS target, and `just test-all` is
  green (EXIT=0; 11 QEMU lanes; `dma buffer ok` in all 6 boots that run it, on the changed
  flush path; the memory record byte-identical to P1's).
- **Found in P2, for P5 (measured in QEMU 11.1.1's source).** Its `emmc` model is
  byte-addressed up to 2 GiB (the capacity bit is set only above), so the lane's image must be
  larger than 2 GiB: the driver drives sector-addressed cards only, like every card of this
  generation. The model offers HS26 and HS52 only, so the lane runs HS52. CMD23 needs the
  card's spec version 3.01, which the `emmc` model sets. The SDHCI model raises transfer
  complete after a busy response, takes END on a transfer descriptor, and reads a zero length
  as 64 KiB.
- **YELLOW for P6 (found in P2): where the DLL locks.** The core locks the DLL at the
  operating clock (187.5 MHz); the mainline driver locks it at 52 MHz and raises the clock
  after. The board decides: if the lock at 187.5 MHz fails, the layer locks at 52 MHz first.
- **P3 PCI ECAM source — ✅ done 2026-09-25.** `source/libs/nexus-pci` (pure, `no_std`,
  `forbid(unsafe_code)`): `PciHost` reads a `pci-host-ecam-generic` node. That covers the ECAM
  window, the bus range, the windows from `ranges` in CPU addresses (`Node::cpu_address`, new
  in nexus-fdt), the INTx routes from `interrupt-map` under its mask, the coherence, and the DMA
  reach the functions inherit (`Node::child_dma_reach`, new). `plan` walks the root bus
  through `ConfigSpace` (ECAM over `nexus_hal::Bus`). It sizes each memory BAR with decoding
  off and places it largest first at the lowest aligned address of its window, a 64-bit BAR in
  the 64-bit window with a fallback below 4 GiB. It turns memory decoding on and routes each
  pin. Two tightenings of C3, recorded there. Every BAR spans at least a page of its own, so no
  capability for one function reaches another. The plan never enables bus mastering;
  `enable_bus_master` is for the grant. Host bridges keep their decoding, PCI-to-PCI bridges
  are recorded and not crossed, and I/O BARs are refused. init (`bootstrap/pci.rs`) maps each
  host's root-bus configuration space through a capability of its own and plans it. It reads
  the first SD host's capability register through the placed BAR and prints one line, written
  with one call: `init: devices from pci ok (…)`, required in every profile; a malformed host
  gets a FAIL line and is skipped. Proof: nexus-pci 19 tests (QEMU virt's host from its golden
  tree and its swizzled routes; a fixture of eight malformed hosts, each refused by name; the
  planner over a synthetic configuration space for placement, isolation, the 64-bit window,
  multi-function devices, bridges, the refusals, determinism and bus mastering at the grant
  alone; ECAM addressing), nexus-fdt goldens 18. Every lane prints `hosts=1 functions=1 sd=none`.
  A manual boot with `sdhci-pci` + `emmc` printed
  `init: devices from pci ok (hosts=1 functions=2 sd=00:01.0 bar=0x40000000 irq=33 caps=0x057834b4)`.
  That is QEMU's `capareg` read through the placed BAR, and line 33 is the measured route.
  Gates: `just check` green; `just test-all` green (EXIT=0; 11 QEMU lanes; the PCI marker in
  every one of the 18 boots that reach init — the fallback lane's slot-b boots stop before
  init by design, like the tree marker).
- **Found in P3.** For P4: the SD host's window is `PciDevices.sd`. Its grant repeats the
  description init read it with, so the kernel keeps one record, and the grant maps the
  configuration space again to enable bus mastering for that function only. For P5: the
  launcher's `QEMU_EXTRA_ARGS` hook does not reach it through `scripts/qemu-test.sh`, which
  declares its own array of that name, so the lane needs a profile knob for its devices.
- **P4 `blkd` — in progress 2026-09-25, in three commits of one intent each:** **P4a** the
  rename with the retired-name gate, the driver's slots from its owner, the stale texts, and
  `blkd` in the OS service list with its partition gate host-tested in `tests/` — **done
  2026-09-25** (below); **P4b** the
  boot disk from `/chosen`, the backend chosen by the granted device, the SDHCI backend and the
  SD host's grant with bus mastering; **P4c** socd in the core plane before the disk grant and
  `blkd` asking it for its node. The package as first written: rename (crate, service, topology ids/specs/slots/routes, init planes,
  supervision, policy, markers, scripts, docs) with the old-name gate; the driver crates take
  their slots from the owner (no service's slot name inside a driver); backend selection by the
  granted device; `/chosen/nexus,boot-disk` written by nxboot and honoured by init; the
  partition gate host-tested with `test_reject_*` in the new `tests/` (the service had none);
  socd joins the core plane BEFORE the disk grant and `blkd` asks it to bring up its node —
  named by the window its capability carries (socd resolves the node by `reg`), `NotNeeded` on
  QEMU; the stale texts the map found (`remote_blk` "2 s deadline", an unemitted watchdog marker,
  `reply_inbox`, the launcher's `/data` device) corrected; every existing lane green under the
  new name.
- **P4a done 2026-09-25 — the name.** The crate, service id, topology slots/specs/routes,
  init's planes and supervision, the policy row, the markers (`blkd: gpt ok (parts=7)`,
  `blkd: irq endpoint bound`), the selftest routes, the scripts and the living docs say `blkd`;
  ADRs, RFCs, ledgers and the changelog keep the old name as dated records.
  `scripts/check-retired-names.sh` (in `just check`) lists each retired name in every spelling
  it was written in, with its successor and the decision, and fails on any of them in a tracked
  or new file outside those records. It matches exact spellings: the virtio device type
  `VirtioBlkDevice` contains the old name case-folded and is not the service. Its fixture
  self-test catches four weakenings (untracked files, case folding, the history exclusion, the
  spellings). The partition gate is `blkd::gate`, a pure module on the kernel-attributed
  sender id; `tests/gate.rs` checks the whole matrix — six senders, ten partition selectors,
  nine op codes — against the grants ADR-0044 and RFC-0089 §12.5 write down, plus two
  `test_reject_*` cases, and four mutations of the gate each fail it. `blkd` joined
  `config/os-services.txt` (diag-os and the dependency gate cover it). The virtio-blk driver
  takes its watchdog pair from its owner, and the ladder now requires `blk: watchdog on` —
  the proof that the pair the owner hands in arms (it was emitted, never required). The four
  stale texts are corrected. Proof: `just check` green; `just build-os-workspace` 0 warnings;
  `ci-os-smp1` green; `ci-os-visible` green (desktop 14.67 % non-black, diff vs splash 31.77);
  `just test-all` green (EXIT=0; 11 QEMU lanes; `blkd: gpt ok` and `blk: watchdog on` in all
  18 boots that reach init, the old name in none).
- **Found in P4a.** For P4b: `policies/base.toml` still grants `device.mmio.blk` to `statefsd`
  and `vfsd` — both have been block-plane clients since TASK-0315, so the policy names three
  possible holders of the disk and ADR-0044 names one; policyd still emits a
  `mmio statefsd-blk` decision line for the old owner. P4b rewrites who may hold the disk
  (`device.mmio.blk` for a virtio disk, `device.mmio.mmc` for an SD host, `blkd` only) with a
  reject test. For P4c: `slots::blkd` says the owner makes no outbound call; asking socd is one.
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
- **YELLOW (found in P1): the PLIC layer's line bound is QEMU's.** `hal::plic::MAX_IRQ` = 95
  bounds `IrqId` and the device descriptor's line, but the K1 PLIC has `riscv,ndev = 159` and
  the eMMC host is line 101 — the board's eMMC grant would be refused today (a TASK-0245
  residual: `plic_ndev()` is read from the tree, the bound is not). P6 derives the bound from
  `riscv,ndev` and sizes the enable bitmap for the controller, before the first board grant.
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
