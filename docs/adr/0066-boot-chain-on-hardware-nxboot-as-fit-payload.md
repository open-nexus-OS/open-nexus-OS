# ADR-0066: On hardware the booted chain is boot ROM → vendor SPL → OpenSBI → nxboot as the FIT payload — no vendor U-Boot in the booted system

- Status: Proposed
- Date: 2026-09-22
- Links:
  - Tasks: `tasks/TASK-0260-*` (image head + fastboot as the flasher protocol),
    `tasks/TASK-0260B-*` (nxboot as the FIT payload), `tasks/TASK-0246B-*` (nxboot's SDHCI reader)
  - RFCs: `docs/rfcs/RFC-0098-board-support-contract-fdt-truth-boot-chain.md` (C6),
    `docs/rfcs/RFC-0089-*` (nxboot, A/B, BSB — unchanged semantics)
  - Related ADRs: `docs/adr/0059-first-stage-boot-chain-nxboot-handoff.md` (nxboot on QEMU),
    `docs/adr/0058-*` (boot-selection block)
  - Measurements: `docs/board/measurements/2026-09-22-stock-system/README.md` (boot flow, load
    addresses), `resources/board/bpi-f3/PROVENANCE.md` (the vendor pieces)

## Context

The board's boot ROM loads a first-stage loader from the medium named by an 80-byte
`bootinfo` header (SD or eMMC), the SPL trains DDR and loads two FIT images from fixed
partitions — `opensbi` (OpenSBI `fw_dynamic`, load address `0x0`) and `uboot` (the vendor
U-Boot proper, load address `0x0020_0000`, with one DTB per board variant) — and OpenSBI jumps
to the `uboot` image in S-mode with the DTB in `a1`. The vendor U-Boot then runs a scripted
environment (`bootcmd=run autoboot` → load kernel/dtb/initrd from an ext4 `bootfs` → `booti`).

We already own a first-stage loader: nxboot (ADR-0059, RFC-0089) — A/B slots, GPT, NXBD
verification, a measured handoff record, SBI reset on failure. On QEMU it is the `-kernel`
payload. The question is which stages of the vendor chain stay when the board boots our OS.

The user's rule for this track: always the target architecture, never a "dev SD card" or
chainload detour.

## Decision

- **The booted chain on hardware is: boot ROM → vendor SPL (DDR init only) → OpenSBI →
  nxboot → kernel.** nxboot is the image in the FIT's `uboot` slot (load `0x0020_0000`, our
  FIT built by `scripts/build-fit.sh` with `mkimage` from OpenSBI + nxboot + our dtb); the
  vendor U-Boot proper is **never** part of a booted Open Nexus OS system.
- The vendor U-Boot survives in exactly one role: the host-side flashing vehicle staged into
  RAM by `just board-flash` (TASK-0327). Nothing it does is trusted or observed by the OS.
- The SPL and OpenSBI are pinned vendor builds fetched at setup time
  (`scripts/fetch-board-inputs.sh`, SHA-256 per piece, provenance + licenses recorded); each is
  a separate program in its own partition, never linked to ours. Building them from source at
  the pinned commits is a follow-up, not a precondition.
- nxboot reads the boot medium through the same `BlockDevice` readers as `blkd`
  (virtio on QEMU, SDHCI on the board; ADR-0067), keeps its A/B + GPT + NXBD flow, passes the
  DTB through unchanged except for `/chosen/nexus,*` (RFC-0098 C2), and on QEMU is the only
  fw_cfg reader.
  (implemented 2026-09-25, TASK-0246B P1: the SDHCI core in PIO, reading and writing the BSB;
  the boot disk is the first candidate — virtio, SD hosts in the tree, SD hosts behind PCI —
  carrying a valid BSB, and its record goes to `/chosen/nexus,boot-disk`. 2026-09-26, P2: on
  the board the loader brings the eMMC host's node up and reads its `io` clock with the SoC
  glue library `socd` runs — before any service exists, RFC-0106's loader clause — and the
  tree's `no-mmc` flags keep the microSD slot and the SDIO host out of the rule)
- `nx image` builds ONE image for QEMU and the board: the boot-ROM head (`bootinfo`, `fsbl`,
  `env` placeholder, `opensbi`, the FIT) prepended to the layout SSOT's volumes; `fastboot`
  writes it partition by partition.
  (Amended 2026-09-26, TASK-0260 P0, measured: `fastboot` writes the image byte for byte through
  the vehicle's raw writes, not partition by partition. The vehicle's `flash gpt` builds its own
  GPT from a JSON, typing every partition basic data, and our stages find partitions by name and
  type. Sector 0 carries a protective MBR next to the boot-ROM header, since U-Boot's GPT driver,
  and so the SPL, sees no GPT without one. The head is GPT partitions 1–4 at the vendor's
  offsets, and the FIT's slot keeps the SPL's name `uboot`.)
- Out of scope: signed FIT / secure boot (follow-up once the chain runs), SPI-NOR boot, the
  vendor's `env`/`bootfs`/`rootfs` (never written).

## Consequences

- **Positive**: one loader we build, test on QEMU and verify on the board; the measured
  handoff record and A/B semantics of RFC-0089 hold on hardware; no second configuration
  language (U-Boot env) and no bootfs filesystem in the boot path.
- **Negative / accepted cost**: nxboot grows a FIT-slot entry (load address, relocation from
  `0x0020_0000`), an SDHCI reader and `/chosen` writing; the DDR-init stage remains a vendor
  binary until a source build lands; a broken nxboot on eMMC needs the FDL key + `board-flash`
  to recover (the microSD stock system is the desk fallback, the boot ROM tries it first).
- **Follow-ups**: TASK-0260 / 0260B / 0246B (this chain), TASK-0327B (the lane that proves it
  over serial), source builds of SPL + OpenSBI, signed FIT.

## Alternatives considered

- **Keep the vendor U-Boot and chainload nxboot from its environment** — rejected: two
  loaders, two truths (U-Boot env vs BSB), a stage we neither build nor verify, and every
  boot-time measurement would start after U-Boot's own delays.
- **UEFI via the vendor U-Boot's `bootefi`** — rejected: a third loader interface, no A/B or
  measured handoff, and it exists only to serve a foreign OS model.
- **Replace the SPL too** — deferred: DDR training is board-specific vendor code; the SPL is
  a separate binary the boot ROM verifies by its header, and replacing it buys nothing for the
  first picture.
- **A dev SD card with the vendor chain and our kernel as "Image"** — rejected by the user's
  rule: an interim chain would be built, proven and then torn down.
