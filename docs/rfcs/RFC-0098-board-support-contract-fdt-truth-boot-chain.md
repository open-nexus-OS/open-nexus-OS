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
- **Phase 1 (kernel platform from the FDT, PIE, init discovery, `/chosen`)**: ⬜ — TASK-0245, 0245B
- **Phase 2 (physical memory from the FDT, page-frame allocator)**: ⬜ — TASK-0286 (M1)
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
| `nexus,boot-profile` | selftest/lane profile name | fw_cfg `opt/org.open-nexus/selftest-profile` (read by nxboot ONLY) | BSB target / default |
| `nexus,boot-slot` | `a` / `b`, the slot nxboot chose | its own selection (RFC-0089) | same |
| `nexus,display-mode` | a REQUEST (`WxH`), never the authority | fw_cfg `display-mode` | absent (EDID decides) |
| `nexus,boot-record` | the measured handoff record itself (60 bytes, ADR-0059 v1 layout) | nxboot | nxboot |

The kernel's syscalls 45 (`BOOT_MODE`) and 50 (`BOOT_DISPLAY_MODE`) read `/chosen`; every
fw_cfg read in the kernel is deleted (Phase 1). 50 is deleted in Phase 5 when gpud owns the
mode. Init and services read the FDT through a read-only VMO the kernel exposes (`device.fdt`
grant, `nexus-service-topology` slot), never through a syscall per value.

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
| devices for init | every node with a `compatible` init knows (virtio-mmio by `device_id`, SDHCI, DPU/HDMI, USB, GMAC, GPU, RTC) with `reg` + `interrupts` | virtio window | the SoC nodes |

CLINT MMIO is never touched from S-mode. The kernel and nxboot are linked position-independent
and relocate themselves on entry (`R_RISCV_RELATIVE`); the load address comes from the FIT
(board) or QEMU's `-kernel` placement — no link-time machine constant.

### C4 — Physical memory (Phase 2 = M1 of target picture M)

The page-frame allocator owns every `/memory` bank not reserved; the fixed windows
(`USER_VMO_ARENA_*`, `KERNEL_PAGE_POOL_*`) and `VmoPool` are deleted. A VMO becomes a page-backed
object with a backing kind; `contiguous-DMA` is the kind DMA masters get. **Coherence rule:** a
node without `dma-coherent` is non-coherent — `DmaBuffer` performs Zicbom maintenance around
every transfer, or maps the buffer non-cacheable through Svpbmt where the ISA lists it; QEMU
virt is coherent and the same code path is a no-op there.

### C5 — Storage (Phase 3, ADR-0067)

`virtioblkd` becomes `blkd`: the ONE block owner, one GPT disk, partition-scoped `blockproto`;
its `BlockDevice` backend is chosen by the FDT node it is granted (`virtio,mmio` with
`device_id 2`, or `spacemit,k1-x-sdhci`). nxboot carries the same two readers. The GPT layout
SSOT (`userspace/storage/src/layout.rs`) gains the boot-ROM head partitions on the board.

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
