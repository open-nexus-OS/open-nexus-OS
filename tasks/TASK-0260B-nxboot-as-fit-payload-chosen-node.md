---
title: TASK-0260B nxboot is the FIT payload on the board — `/chosen/nexus,*` written by nxboot, fw_cfg read only there, one image for QEMU and the board
status: In Progress (P0 recut, P1 the FIT and P2 the first eMMC boot ✅ 2026-09-27 — boot ROM → SPL → OpenSBI → nxboot → the verified kernel jump, read from the board's boot trace; the board tree carries what the firmware reads; next P3 the kernel's side, read with RFC-0107 Phase 2; seeded 2026-09-22 at Block 1 P0 as the B part of TASK-0260)
owner: @reliability @runtime
created: 2026-09-22
depends-on:
  - tasks/TASK-0260-provisioning-recovery-v1_0a-host-image-builder-flasher-protocol-deterministic.md
  - tasks/TASK-0246B-nxboot-sdhci-disk-reader.md
  - tasks/TASK-0245-bringup-rv-virt-v1_0b-os-kernel-uart-plic-timer-uartd-selftests.md
follow-up-tasks:
  - tasks/TASK-0251-display-v1_0b-os-fbdevd-windowd-integration-cursor-selftests.md
  - tasks/TASK-0327B-board-proof-lane-serial-ladder-profiles.md
links:
  - Decision: docs/adr/0066-boot-chain-on-hardware-nxboot-as-fit-payload.md
  - Contract: docs/rfcs/RFC-0098-board-support-contract-fdt-truth-boot-chain.md (C1, C2, C6, Phase 4)
  - nxboot: source/boot/nxboot (ADR-0059, RFC-0089); tools: scripts/build-fit.sh (new), scripts/board-flash.sh (TASK-0327)
  - Measurement: docs/board/measurements/2026-09-22-stock-system/README.md ("Boot flow"), resources/board/bpi-f3/PROVENANCE.md (FIT facts: OpenSBI at 0x0, payload at 0x0020_0000, `fdt_1 = k1-x_deb1`), docs/board/measurements/2026-09-26-boot-medium/README.md (TASK-0260 P0: the SPL finds `opensbi`/`uboot` through the GPT and selects a FIT configuration by name)
  - Playbook: CLAUDE.md
---

## RECUT 2026-09-27 — P0: the FIT measured, the payload's footprint, the first board attempt read from the trace

Measured before building (the pinned vendor FIT, `dumpimage -l`/`fdtget`, and nxboot's own link):

- The vendor FIT holds U-Boot and one tree per board variant. OpenSBI is not in it: the SPL
  loads OpenSBI from its own partition, `opensbi`. So our FIT holds **nxboot + our tree**, and
  OpenSBI stays the pinned `fw_dynamic.itb` (the earlier "OpenSBI + nxboot + dtb" is corrected
  here and in ADR-0066).
- The payload's shape: root `#address-cells = <2>`; the image `type = "standalone"`,
  `os = "U-Boot"`, `arch = "riscv"`, `compression = "none"`, `load = <0 0x200000>`, a crc32 hash
  (OpenSBI's next stage is the image with `os = "U-Boot"`). The configurations:
  `default = "conf_1"`, each `{description = "<board name>", loadables, fdt}`, and the SPL picks
  one by the board's name (`k1-x_deb1`). Our one configuration carries that name and is the
  default, so both paths take it.
- **The payload must cover its whole footprint.** The SPL places the tree right after the
  payload's bytes and then grows it in place. In U-Boot v2022.10 (the vendor's base),
  `common/spl/spl_fit.c` sets `image_info.load_addr = spl_image->load_addr + spl_image->size`
  and then calls `fdt_shrink_to_minimum(fdt, 8192)`. OpenSBI grows it again in place: in v1.3,
  `lib/utils/fdt/fdt_fixup.c` calls `fdt_open_into(fdt, fdt, fdt_totalsize(fdt) + 1024)` for its
  reserved memory, and +1024/+32 for the other fixups.
  - nxboot's `.bss` is `NOLOAD`: its heap (192 KiB), the trace's capture buffer (64 KiB) and its
    stack (16 KiB). A flat image ends at `__load_end` (130 KiB).
  - So the tree would sit inside `.bss`, zeroed by nxboot's first instructions, and the loader
    would reset before it has a console or a disk.
  - So the FIT's payload is nxboot's flat image padded to `__image_end` (416 KiB), its whole
    footprint, read from the ELF's own symbols.
- nxboot needs no platform split: it reads fw_cfg only when the tree lists a
  `qemu,fw-cfg-mmio` node (TASK-0245), and everything else already comes from the tree.
- The eMMC's boot configuration needs no change. Measured from the stock system (debugfs
  `ext_csd`): `PARTITION_CONFIG` [179] = `0x00` (BOOT_PARTITION_ENABLE 0), `BOOT_SIZE_MULT` 32
  (4 MiB), `BOOT_INFO` `0x07`.
  - That is the state the vendor's own eMMC flow leaves: `clear_emmc()` in its fastboot code
    sets `mmc_set_part_conf(mmc, 0, 0, 1)`, and nothing after it enables a boot partition.
  - So the boot ROM reads boot0 by partition access, not by the eMMC's boot operation.

### Packages

- **P1 — the FIT ✅ 2026-09-27.**
  - `config/board/bpi-f3/nexus.its` (the measured shape, one configuration named `k1-x_deb1`).
  - `scripts/build-fit.sh`:
    - nxboot from the last OS build, padded to its footprint;
    - the tree from `scripts/build-board-dtb.sh`;
    - `mkimage` with no build time recorded (`SOURCE_DATE_EPOCH=0`);
    - the shape checked with `dumpimage`, the size against the partition.
  - `just board-image` always builds the FIT and puts it into `uboot`.
  - nxboot's first line, `nxboot: platform=<root compatible> tree=0x<a1> size=<bytes>`, printed as
    soon as it has a console. It records which tree reached `a1` and where it lies (R1): on the
    board, where the SPL put it.
    - It is the first line in the trace, like every loader line.
    - It replaces RFC-0098 C8's planned `platform=<compatible> slot=<s>`: the slot is known only
      after the boot-selection block and is already on `nxboot: jump slot=<s>`, and the hart is
      always 0 once nxboot has moved itself there.
  - The build checks that the FIT's payload covers the footprint.
  - Proof:
    - two builds byte-identical (`c820eefe…`);
    - five mutations each refused by the build: the load address, no padding, the default
      configuration, a FIT larger than its partition, no nxboot;
    - the harness's new requirement run standalone: PASS, FAIL, SKIP;
    - `just check`, `ci-os-smp1` and `just test-all` EXIT=0 (12 QEMU runs, every trace contract
      with the platform line first).
- **P2 — the first eMMC boot ✅ 2026-09-27** (`docs/board/measurements/2026-09-27-first-emmc-boot/`).
  Each attempt:
  1. flash (FDL) and read back exactly;
  2. a baseline `just board-log`: the trace is empty;
  3. the microSD out, reset: the boot ROM boots the eMMC;
  4. the microSD in, reset;
  5. `just board-log`.
  - **Attempt A:** no boot kept, USB silent throughout (no fallback to the boot ROM's or the
    SPL's download mode), only the power LED.
    - Measured without changing anything:
      - the eMMC boot config is the vendor flow's own;
      - the SPL takes our FIT (its strings);
      - the pinned OpenSBI's K1 code reads nothing from the tree and it never reads
        `riscv,isa`.
    - **But OpenSBI runs on our tree, and our CLINT had no `interrupts-extended`.** OpenSBI
      v1.3's ACLINT timer then serves no hart, `init_coldboot` prints "timer init failed" and
      calls `sbi_hart_hang()`: a silent stop before our loader.
    - The fix: the CLINT names every hart's MSIP (3) and MTIP (7), and the UART also carries
      `spacemit,pxa-uart`, the name the firmware's console matches.
    - Held by a host test on the board golden (both mutations killed). `just board-goldens` in
      `just check` keeps that golden byte-equal to `board.dts`: it was never checked before,
      and `dtc` is now a core dependency and in CI.
  - **Attempt B, with the fixed tree:** the loader's six lines from the board.
    - The platform, and the tree at `0x268000`: R1 measured, right after the padded payload,
      where an unpadded one would have put it inside `.bss`.
    - The eMMC found through `mmc@d4281000`.
    - The boot selection block, verify ok, `/chosen`.
    - The jump to the verified kernel at `0x400000`.
    - One boot kept, so no reset loop.
- **P3 — the kernel on the board.** Read with RFC-0107 Phase 2 (TASK-0327B P2, the kernel's
  console ring and the OS writer): how far the kernel comes, then what stops it. Done when the
  board's trace reaches `init: ready`, with this ledger's Definition of Done.

## Context (measured 2026-09-22)

The SPL loads two FITs from the `opensbi` (1 MiB) and `uboot` (2 MiB) partitions; OpenSBI
jumps to the `uboot` FIT's image (load `0x0020_0000`) in S-mode with the FIT's selected DTB in
`a1`. The vendor payload is U-Boot; ours is nxboot. On QEMU nxboot is the `-kernel` payload
and the only place that may read fw_cfg (RFC-0098 C2).

## Context added 2026-09-26 (TASK-0260 P0, measured)

The FIT goes into the head partition the SPL knows as `uboot` (2 MiB at 2 MiB; partition 2 since
TASK-0260 P2): the SPL loads it by that name (`CONFIG_SYS_LOAD_IMAGE_SEC_PARTITION_NAME`, the vendor's
defconfig), whatever the slot holds. The SPL picks a FIT configuration by name (`Boot from fit
configuration %s`) — which name it asks for, and so which configuration our single-config FIT must
answer (or its default), is measured with the first boot (TASK-0260 P3). (Since P1, `just
board-image` always builds the FIT and puts it into the board image; `just board-flash --plan`
writes it.)

## Goal

- `scripts/build-fit.sh` (`mkimage`): `config/board/bpi-f3/nexus.its` → a FIT with image
  `nxboot` at `0x0020_0000` (the slot the SPL loads) and our `board.dtb` (padded with `/chosen`
  headroom); `nx image` places it in the FIT partition of the layout SSOT (TASK-0260's head).
- nxboot: `platform/{qemu,board}.rs` — on QEMU read fw_cfg (`selftest-profile`,
  `display-mode`) and write `/chosen/nexus,boot-profile` / `nexus,display-mode`; on both write
  `nexus,boot-slot` and `nexus,boot-record`; hart lottery from `/cpus`; self-relocation from
  the FIT load address (position-independent, TASK-0245); disk via TASK-0246B; SBI SRST on
  failure as today.
- The kernel's `/chosen` readers (TASK-0245) are the only consumers.
- Proof: QEMU A/B + NXBD lanes unchanged with the `/chosen` path; **board**: `nxboot:
  platform=spacemit,k1-x slot=a` → kernel banner → `KSELFTEST: platform from fdt ok` →
  `init: ready` on the serial console — measured R1 (which DTB reached `a1`) recorded here.

## Non-Goals

Signed FIT / secure boot (follow-up), SPI-NOR boot, replacing the SPL or OpenSBI (ADR-0066),
recovery over the USB gadget (TASK-0261, parked).

## Definition of Done

FIT built reproducibly (`sha256` stable across two builds); `just board-flash` writes the
image; the board's serial ladder reaches `init: ready`; ADR-0066 Accepted; RFC-0098 Phase 4 ✅;
`docs/board/bpi-f3.md` boot section rewritten from "vendor chain" to "our chain".
