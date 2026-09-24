# RFC-0098: Board support contract — the FDT is the one hardware truth, and the boot chain on hardware

- Status: In Progress (Phase 0 ✅ 2026-09-22; seeded 2026-09-22, Block 1 P0 of the hardware fast track)
- Owners: @kernel-team / @runtime / @tools-team
- Created: 2026-09-22
- Last Updated: 2026-09-22 (Phase 0 Implemented)
- Links:
  - Tasks (execution + proof, in lane order): `tasks/TASK-0244-*` (FDT library),
    `tasks/TASK-0245-*` (kernel platform from the FDT), `tasks/TASK-0245B-*` (SoC clock/reset/
    pinmux/power), `tasks/TASK-0286-*` (physical memory from the FDT + page-frame allocator),
    `tasks/TASK-0246-*` / `tasks/TASK-0246B-*` (SDHCI + the one block owner; nxboot reader),
    `tasks/TASK-0260-*` / `tasks/TASK-0260B-*` (image head + fastboot; nxboot as FIT payload),
    `tasks/TASK-0250-*` / `tasks/TASK-0251-*` (display controller scanout, mode authority)
  - ADRs: `docs/adr/0066-boot-chain-on-hardware-nxboot-as-fit-payload.md`,
    `docs/adr/0067-one-block-owner-backend-selected-by-fdt.md`
  - Amends: `docs/rfcs/RFC-0074-display-mode-authority-fwcfg.md` (display-mode authority leaves
    fw_cfg and the kernel), narrows `docs/rfcs/RFC-0017-device-mmio-access-model-v1.md`
    (device discovery) and `docs/rfcs/RFC-0089-*` (nxboot on real media)
  - Related: `docs/rfcs/RFC-0085-kernel-owned-va-allocation.md`, `docs/rfcs/RFC-0093-*`
    (stage fence; unchanged), `docs/board/bpi-f3.md`,
    `docs/board/measurements/2026-09-22-stock-system/README.md` (the numbers this RFC is built on)

## Status at a Glance

- **Phase 0 (FDT library, `nexus-fdt`, nxboot owns `/chosen`, the kernel reads the tree)**: ✅ 2026-09-22 — TASK-0244
- **Phase 1 (kernel platform from the FDT, PIE, init discovery, `/chosen`)**: 🟨 — TASK-0245 ✅ 2026-09-22 (QEMU: every profile prints the platform, image and discovery markers; the board's serial proof arrives with TASK-0327B once B1.6 boots it); TASK-0245B (SoC clocks/resets/pinmux/power) open
- **Phase 2 (physical memory from the FDT, page-frame allocator)**: ✅ 2026-09-24 — TASK-0286 (M1): high half + direct map, frame pool, page-backed VMOs, `vmo_runs`, coherence in the device capability, user Zicbom, `DmaBuffer`, `mm_stats` (QEMU: every profile; the board's serial proof arrives with TASK-0327B once B1.6 boots it)
- **Phase 3 (one block owner, SDHCI, nxboot reader)**: ⬜ — TASK-0246, 0246B
- **Phase 4 (boot chain: image head, fastboot, nxboot as FIT payload)**: ⬜ — TASK-0260, 0260B
- **Phase 5 (display controller scanout, mode authority = gpud)**: ⬜ — TASK-0250, 0251

Definition: "Complete" = every phase's proof gates are green on QEMU **and** on the board
lane (`TASK-0327B`); the last QEMU-only literal is gone from the tree.

## Scope boundaries (anti-drift)

- **This RFC owns**: what the FDT must say, who writes `/chosen`, what the kernel/init/nxboot
  may read from it and from nowhere else, the position-independence of kernel and nxboot, the
  DMA-coherence rule, the block-owner rule, the display-mode authority, the boot chain on
  hardware, and the marker contract that proves each.
- **This RFC does NOT own**: the drivers' internals (SDHCI, DPU, GPU, USB — their ledgers),
  the memory object model beyond the page-frame allocator (RFC-0100, target picture M), the
  proof-lane mechanics on the board (`TASK-0327B`), firmware provenance policy (hardware rule 3
  in the order file), signed FIT / secure boot (a follow-up once nxboot rides the FIT).

### Relationship to tasks (single execution truth)

Each phase is one or two ledgers with their own stop conditions; this RFC links them and
records the contract they implement. Measurements that changed the contract are dated.

## Context

Every platform value in the tree is a QEMU-`virt` literal: CLINT `0x0200_0000`, PLIC
`0x0c00_0000`, UART `0x1000_0000`, the virtio-mmio window `0x1000_1000 × 8`, the goldfish RTC
`0x0010_1000`, `TICKS_PER_US = 10`, IRQ numbers derived from a virtio slot index, boot-mode and
display-mode read from QEMU's fw_cfg (syscalls 45/50, RFC-0074), physical memory as fixed
windows above `0x8000_0000`. nxboot passes the DTB pointer through untouched and nobody parses
one. Six of seven drivers are virtio.

The reference board (`docs/board/bpi-f3.md`) has none of those values: PLIC at
`0xe000_0000` with 159 sources, UART `spacemit,pxa-uart` at `0xd401_7000`, timebase 24 MHz,
Sstc, **RAM at physical 0** in two banks (0–2 GiB, 4–6 GiB), three SDHCI hosts, a display
controller with HDMI, a GPU, and DMA masters that are **not cache-coherent** (Zicbom/Svpbmt
present, the stock kernel bounces through swiotlb). Measured 2026-09-22 from the running stock
system, archived in `docs/board/measurements/2026-09-22-stock-system/`.

Two truths would be one too many. The device tree already exists on both machines — QEMU
generates one, the board's boot chain passes one — so the contract is: **the FDT is the one
hardware truth**, and everything that used to be a literal becomes a value read from it.

## Goals

1. No MMIO address, IRQ number, clock frequency, hart count or memory range in code outside
   the FDT parser and its direct consumers; a grep gate enforces it.
2. One binary each for kernel and nxboot, position-independent, booting QEMU `virt` and the
   board from the same source with no cfg.
3. The boot chain on hardware is ours from OpenSBI onward (ADR-0066): nxboot is the FIT
   payload, U-Boot is only the host-side flashing vehicle.
4. One block owner with a backend the FDT selects (ADR-0067); the GPT plane and every consumer
   above it are untouched.
5. The display mode has one authority — gpud, from EDID on the board and from the virtio
   display-info on QEMU — and the fw_cfg syscall path is deleted.
6. Every phase proves itself on QEMU first and then on the board with the SAME markers.

## Non-Goals

Runtime device hotplug or overlays; ACPI; a generic driver-binding framework (drivers are
services that read their node; init grants MMIO by compatible as today via RFC-0017);
per-board cfgs or feature flags; U-Boot interoperability beyond the fastboot flash path.

## Contract

### C1 — The FDT and its sources

- **QEMU `virt`**: the DTB QEMU generates (`-machine dumpdtb` is the golden for host tests).
- **Board**: `config/board/bpi-f3/board.dts`, OUR file, derived from the mainline SoC dts
  (GPL-2.0 OR MIT — used under MIT) and the measured facts; it contains only the nodes we
  consume. Compiled to a dtb by `dtc` at image-build time and carried inside the FIT
  (ADR-0066). The vendor kernel's tree is never copied into the repo.
- The DTB reaches the kernel in `a1` exactly as the previous stage handed it over; nxboot is
  the only stage that MODIFIES it, and only its `/chosen` node.

### C2 — `/chosen/nexus,*` is nxboot's, and only nxboot's

nxboot writes (in-place, with headroom reserved at FIT build time):

| Property | Meaning | Source on QEMU | Source on the board |
|---|---|---|---|
| `nexus,boot-mode` | `proof` / `interactive` — the kernel's marker folding (syscall 45) | fw_cfg `opt/org.open-nexus/selftest-mode` (read by nxboot ONLY) | absent (= proof, raw markers) |
| `nexus,boot-profile` | selftest/lane profile name | fw_cfg `opt/org.open-nexus/selftest-profile` (read by nxboot ONLY) | BSB target / default |
| `nexus,boot-slot` | `a` / `b`, the slot nxboot chose | its own selection (RFC-0089) | same |
| `nexus,display-mode` | a REQUEST (`WxH`), never the authority | fw_cfg `display-mode` | absent (EDID decides) |
| `nexus,boot-record` | the measured handoff record itself (60 bytes, ADR-0059 v1 layout) | nxboot | nxboot |

The kernel's syscalls 45 (`BOOT_MODE`) and 50 (`BOOT_DISPLAY_MODE`) read `/chosen`; every
fw_cfg read in the kernel is deleted (Phase 1). 50 is deleted in Phase 5 when gpud owns the
mode. Init and services read the FDT through a read-only VMO the kernel exposes (init: the
`VmoRo` alias the kernel injects at `nexus_abi::INIT_DEVICE_TREE_SLOT`; services: the same alias
pinned into their declared `NamedSlot::DeviceTree`), never through a syscall per value. Since
TASK-0245 P4 the harness reads its boot mode/profile there and init discovers virtio transports
and the RTC there (`init: devices from fdt ok (…)`, required in every profile).

### C3 — What the kernel derives, and how

| Value | FDT source | QEMU virt | Board |
|---|---|---|---|
| hart list, count, topology | `/cpus`, `cpu-map` | 4 (or `-smp`) | 8, two clusters |
| timebase | `/cpus/timebase-frequency` | 10 MHz | 24 MHz |
| timer | `riscv,isa-extensions` has `sstc` → `stimecmp`; else SBI TIME | SBI (or Sstc with `-cpu max`) | Sstc |
| IPI | SBI IPI extension (both) | | |
| PLIC | compatible `riscv,plic0` / `sifive,plic-1.0.0` / `spacemit,k1-plic`: `reg`, `riscv,ndev`, `interrupts-extended` → S-mode context per hart | `0x0c00_0000`, 53 sources | `0xe000_0000`, 159 sources |
| console | `/chosen/stdout-path` → node: `ns16550a` (1-byte stride) or `spacemit,pxa-uart` / `intel,xscale-uart` / `snps,dw-apb-uart` (4-byte stride) | `0x1000_0000` | `0xd401_7000` |
| memory | every `/memory@*` `reg`, minus `/reserved-memory` and the kernel's own image | one bank at `0x8000_0000` | two banks at `0x0` and `0x1_0000_0000` |
| devices for init | every node with a `compatible` init knows (virtio-mmio by `device_id`, SDHCI, DPU/HDMI, USB, GMAC, GPU, RTC) with `reg` + `interrupts`; the PLIC line travels INSIDE the device capability (`cap_query` → `irq`), no driver derives it from an address | `virtio,mmio` nodes, `google,goldfish-rtc` | the SoC nodes |

CLINT MMIO is never touched from S-mode. The kernel and nxboot are linked position-independent
and relocate themselves on entry (`R_RISCV_RELATIVE`); the load address comes from the FIT
(board) or QEMU's `-kernel` placement — no link-time machine constant.

**Amended 2026-09-24 (TASK-0246 P0): a PCI host bridge is a device source.** Measured
(`docs/board/measurements/2026-09-24-emmc-sdhci`): QEMU's only SD host on `virt` is
`sdhci-pci` behind `pci-host-ecam-generic`, and nothing before the OS assigns its BAR. A
`pci-host-ecam-generic` node is enumerated like a list of nodes: its buses read through ECAM,
every function's memory BARs sized and assigned lowest-first from the node's 32-bit memory range
(a 64-bit BAR from the 64-bit range), memory decode and bus mastering enabled, INTx routed
through the node's `interrupt-map` to the PLIC line. The result is the same `DeviceMmio`
capability (window, line; coherence and DMA reach from the host node) and the same grant by
class. ONE pure, host-tested planner does it for nxboot and init, so both see one assignment.
The board's PCIe (a DesignWare host with link training) is a driver of its own later; this is
the generic ECAM host only.

### C4 — Physical memory (Phase 2 = M1 of target picture M)

The page-frame allocator owns every `/memory` bank not reserved; the fixed windows
(`USER_VMO_ARENA_*`, `KERNEL_PAGE_POOL_*`) and `VmoPool` are deleted. A VMO becomes a page-backed
object with a backing kind; `contiguous-DMA` is the kind DMA masters get. **Coherence rule
(corrected 2026-09-24, TASK-0286 P4b — see below):** a device is non-coherent when its node or
an ancestor bus carries `dma-noncoherent`; `DmaBuffer` performs Zicbom maintenance around every
transfer for such a device; QEMU virt is coherent and the same code path is a no-op there.

**Amended 2026-09-22 (TASK-0286 P0): the kernel lives in the high half.** Measured: the kernel
identity-maps everything it owns as GLOBAL pages, and RAM starts at physical 0 on the board — an
identity-mapped kernel and its frames would sit inside the user VA range. Contract: a direct map
`KVA = PHYS_OFFSET + PA` (`PHYS_OFFSET = 0xffff_ffc0_0000_0000`, the Sv39 kernel half) of every
bank and every device window; the kernel image, the page tables, the frames and the tree are
reached through it and nothing kernel-owned lies below it; "user address" means
`va < KERNEL_VA_BASE`. The boot switch (fixups at the load PA → early platform at PA → a boot
table of 1 GiB pages → SATP → the high half → fixups again with the high base) is part of the
kernel's own entry, on QEMU and on the board alike; `phys_to_virt`/`virt_to_phys` are the one
seam. RFC-0085's user window is unchanged. **Implemented 2026-09-23 (TASK-0286 P2):** `phys.rs`
(seam + boot table, host-tested), the two-phase entry in `neuron-boot`, the secondary stub's
switch, `kernel_layout.rs` over banks + windows, the identity map deleted; `KINIT: kernel high
half (base=0xffffffc0…)` on every boot. **Implemented 2026-09-23 (TASK-0286 P3a):** the VMO is
a page-backed object (`mm/vmo.rs`: `Anon`, `Contiguous`, `Fixed`); `sys_vmo_create` takes the
kind in arg 2 (bit 0 = one physically contiguous block, `nexus_abi::vmo_create_contiguous`);
`cap_query` reports a physical base only for a one-run object. `VmoPool` and the arena are gone.
**2026-09-23 (P3b):** the last fixed windows (init loader pages, spawn stacks, the bootstrap
identity VMO) are frames; `scripts/check-no-fixed-windows.sh` keeps it so; the machine memory
is a lane knob (`QEMU_MEM`).

**Amended 2026-09-24 (TASK-0286 P4a): one door for a physical address.** Measured: seven DMA
consumers learned a VMO's physical base through `cap_query`, and gpud's resource backings,
windowd's framebuffer (1280 × 9600 × 4 = 49 MiB, a 64 MiB block) and app-host's surfaces were
contiguous only because `cap_query` can name one run — the virtio-gpu backing is a
scatter-gather list, and no device ever reads an app surface (windowd copies it with
`vmo_read`). Contract: a physical address leaves the kernel only through **`vmo_runs`
(syscall 60)** — `(slot, offset, len, out, max)` writes the `(pa, len)` runs that cover the
byte range of a writable `Vmo` the caller holds with MAP, physically adjacent runs merged, at
most `max ≤ 256` per call, all or nothing (a range that needs more runs is refused, never
truncated). Authority: the caller holds a device capability (`DeviceMmio`) — a physical
address is good for nothing but programming a bus master, so a task without a device has none
to learn — and a read-only alias never yields one (a device could write through the address).
`cap_query` reports a base only for `DeviceMmio` (its register window); for a VMO it reports
0. `contiguous-DMA` stays the kind for memory a device addresses by ONE base (virtio queues,
command and response pools, request buffers); memory a device reads through a list is `Anon`.

**Amended 2026-09-24 (TASK-0286 P4b): coherence travels with the device.** Measured: the rule
as first written ("a node without `dma-coherent` is non-coherent") is backwards for RISC-V — the
binding's default is coherent and a non-coherent master is marked `dma-noncoherent` (the
mainline K1 tree puts it on its `soc` bus; QEMU virt's tree carries neither property, so the
first rule would have made every virtio device non-coherent). Contract: init reads the property
walking from the device node up to the root (`nexus_fdt::Node::dma_coherent`: the nearest
`dma-noncoherent` or `dma-coherent` decides, none means coherent) and mints it INTO the device
capability (`device_cap_create` arg 4 bit 0; any other bit is refused), exactly like the
interrupt line; `cap_query` reports it (`flags` bit 0) together with the harts' cache-block
size (`cache_block`, the tree's `riscv,cbom-block-size` when the ISA lists `zicbom`, else 0) —
the query grows from 24 to 32 bytes. The kernel enables Zicbom for user mode on every hart whose
tree lists it: `senvcfg.CBCFE = 1` and `senvcfg.CBIE = 01`, so `cbo.inval` executes as a flush —
a driver can write back and drop its own lines but never discard data (user mode gets no
data-destroying primitive). Maintenance is the driver's, through one type: `DmaBuffer`
(`nexus-driverkit`) owns a mapped DMA object (`nexus_abi::DmaVmo`) and moves between CPU-owned
and device-owned by value — `for_device(dir)` cleans (to the device) or flushes (from or both
ways), `for_cpu()` flushes what the device may have written; the CPU cannot touch the bytes
while the device owns them (a compile-time guarantee). A non-coherent device on a machine
without Zicbom is refused (`DmaCoherence::Unmaintainable`); non-cacheable mappings through
Svpbmt are the fallback M-track work if a board ever needs it. The firmware must enable the
same bits for S-mode (`menvcfg`); OpenSBI ≥ 1.3 does — measured on the board in B1.6.

**Amended 2026-09-24 (TASK-0286 P5): what memory is used, and when it ran out.** The frame
pool, the page tables, the VMO table and every address space's region table already hold the
truth; the accounting is a read of them, never a second set of counters. **`mm_stats`
(syscall 61)** answers a fixed, versioned record (`version`, then 64-bit fields: banks, total,
free, reserved, excluded, allocs, frees, exhausted, page-table frames, live objects, their
bytes, their contiguous-DMA bytes, and the caller's own resident bytes and DMA bytes — the
Vmo- and kernel-placed regions of its address space) into a caller buffer of at least the
record's size; a shorter buffer is refused. It needs no authority because it reveals nothing
about another task; another task's residency is the OOM handoff's (M4, TASK-0287) and comes
with its own capability there. `KSELFTEST: mm frames (…)` prints the same record plus the
address-space summary (`spaces`, `rss_sum`, `rss_max`) at the ladder's late fence. **Exhaustion
is an event (RFC-0087 §1):** the defined moment is a request the pool cannot satisfy at all —
an anonymous object that falls back to smaller blocks is not exhausted, and the allocator takes
that fallback internally (`alloc_at_most`) so neither the counter nor the log sees it; the line
`MM: frames exhausted (…) event=exhaust.v1 resource=frames action=refused` is printed on the
1st, 2nd, 4th, 8th … occurrence (bounded under a storm, never silent), the counter holds all.

**Amended 2026-09-24 (TASK-0246 P0): DMA reach travels with the device.** Measured: the K1's
storage masters (the SDHCI hosts, the DWC3) sit on a `storage-bus` whose `dma-ranges` maps only
`[0, 2 GiB)` — the board's second bank at 4 GiB is out of their reach — while the camera, dma,
multimedia, network and PCIe buses TRANSLATE (bus `0x8000_0000…` → CPU `0x1_0000_0000…`). A
DMA buffer lands in reach today only because the pool fills bank 0 first: correct by luck, and
silent corruption once bank 0 is full. Contract: the tree says it (`board.dts` carries the
buses and their `dma-ranges` for every node it has); init reads a device's ranges walking up
from its node (no `dma-ranges` anywhere above means the identity over all memory) and mints them
into the device capability next to the line and the coherence — with more attributes than
argument registers, `device_cap_create` takes a versioned descriptor. A VMO made FOR a device
is allocated within its reach, and `vmo_runs` names the device it answers for: the runs come
back in that device's BUS addresses, a run outside its reach is refused (never truncated, never
silently wrong), and the caller must hold THAT device's capability (tighter than P4a's "any
device").

### C5 — Storage (Phase 3, ADR-0067)

`virtioblkd` becomes `blkd`: the ONE block owner, one GPT disk, partition-scoped `blockproto`;
its `BlockDevice` backend is chosen by the FDT node it is granted (`virtio,mmio` with
`device_id 2`, or `spacemit,k1-sdhci` — corrected 2026-09-24, see below). nxboot carries the same two readers. The GPT layout
SSOT (`userspace/storage/src/layout.rs`) gains the boot-ROM head partitions on the board.

**Amended 2026-09-24 (TASK-0246 P0, measured):** the board tree's compatible is the mainline
`spacemit,k1-sdhci` (the vendor tree's `spacemit,k1-x-sdhci` is not ours); on QEMU the SDHCI
backend is reached through the PCI source (C3: class `0805`, `sdhci-pci`) — one `sdhci` driver
for both, a K1 layer (vendor registers, HS400 enhanced strobe) over the standard core. **The disk
is the medium the boot came from:** nxboot writes `/chosen/nexus,boot-disk` (the node path, or
the PCI function behind a host node) for the device it read the boot volume from, and init
grants exactly that one to `blkd` — the board carries three SD hosts and a stock SD card, and a
choice by address order would be a guess. The operating point is reached without tuning: HS52
(8 bit, SDR), then HS400 enhanced strobe (PHY DLL lock) — the stock system's own mode; ADMA2
32-bit (the host's and the bus's limit). nxboot initialises the card itself (it trusts no
predecessor's controller state), with the same core in PIO mode.

### C6 — Boot chain (Phase 4, ADR-0066)

Boot ROM → vendor SPL (DDR) → OpenSBI (`fw_dynamic`) → **nxboot** as the FIT's `uboot`-slot
image → kernel. `nx image` builds ONE image with the head (`bootinfo`, `fsbl`, `env` placeholder,
`opensbi`, the FIT) and our volumes; `fastboot` is the flasher protocol (TASK-0327). The FIT is
built by `scripts/build-fit.sh` (`mkimage`) from OpenSBI + nxboot + our dtb.

### C7 — Display mode (Phase 5)

gpud is the authority: on the board it reads EDID over the HDMI encoder's DDC and picks the
highest mode the controller supports (1920×1080@60 on this SoC); on QEMU it uses
`ctrl_query_display_info`. windowd asks gpud (the existing handshake, RFC-0093); syscall 50
and the fw_cfg key are deleted; RFC-0074's authority statement is amended to point here.
`/chosen/nexus,display-mode` remains a request a lane can make on QEMU.

### C8 — Proof markers (contracts; registered in the proof manifest)

| Marker | Proves | Phase |
|---|---|---|
| `KSELFTEST: platform from fdt ok (uart=… plic=… tb=…Hz harts=…)` | every platform value came from the dtb | 1 |
| `KSELFTEST: mm frames (banks=… total=… free=…)` | the allocator owns the FDT's memory | 2 |
| `KSELFTEST: vmo runs ok (runs=… deny=3)` | a physical address leaves the kernel only through `vmo_runs`, only to a device holder, never for a read-only alias or past the object | 2 |
| `SELFTEST: dma buffer ok (device=… block=… runs=…)` | the device capability carries its coherence; user-mode Zicbom runs through a `DmaBuffer` with every byte intact | 2 |
| `blkd: backend=<compatible> …` | the block owner bound the FDT-selected device | 3 |
| `nxboot: platform=<compatible> slot=<a\|b>` | nxboot ran as the FIT payload on the board | 4 |
| `gpud: dc scanout ok (WxH@Hz edid)` + `windowd: desktop revealed` | the first picture | 5 |

Gate scripts: `scripts/check-no-platform-literals.sh` (Phase 1), `scripts/check-no-fixed-windows.sh`
(Phase 2). Board lane: `TASK-0327B`.

## Security considerations

- The FDT is TRUSTED input from the boot chain (as the kernel image itself is); it is still
  parsed with bounds on every offset and length — a malformed tree is a boot failure with a
  message, never a wild read. No `unwrap` in the parser; host tests include truncated and
  corrupt trees (`test_reject_*`).
- Only nxboot writes `/chosen/nexus,*`; the kernel treats a missing property as "absent",
  never as a default profile that enables anything.
- MMIO grants stay deny-by-default per class (RFC-0017 + policyd); the FDT adds discovery,
  not authority.
- Firmware blobs reach the image only through the provenance gate (order file, rule 3).

## Alternatives considered

- **A board cfg / feature flag per platform** — rejected: two binaries, two truths, and the
  QEMU lane would stop proving the board path.
- **Keep fw_cfg for QEMU-only knobs** — rejected: nxboot re-expresses fw_cfg in `/chosen`,
  so the kernel has ONE reader; the lanes keep their knobs.
- **Keep the vendor U-Boot and chainload nxboot** — rejected (ADR-0066): two loaders, U-Boot's
  environment as a second truth, and a stage we do not build or verify.
- **Fixed memory windows relocated by the FDT** — rejected: an interim; the page-frame
  allocator is the end state and RAM at 0 in two banks needs it anyway.
