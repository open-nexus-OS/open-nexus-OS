---
title: TASK-0260B nxboot is the FIT payload on the board — `/chosen/nexus,*` written by nxboot, fw_cfg read only there, one image for QEMU and the board
status: Draft (seeded 2026-09-22 at Block 1 P0 as the B part of TASK-0260)
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

## Context (measured 2026-09-22)

The SPL loads two FITs from the `opensbi` (1 MiB) and `uboot` (2 MiB) partitions; OpenSBI
jumps to the `uboot` FIT's image (load `0x0020_0000`) in S-mode with the FIT's selected DTB in
`a1`. The vendor payload is U-Boot; ours is nxboot. On QEMU nxboot is the `-kernel` payload
and the only place that may read fw_cfg (RFC-0098 C2).

## Context added 2026-09-26 (TASK-0260 P0, measured)

The FIT goes into the head partition the SPL knows as `uboot` (#4, 2 MiB at 2 MiB): the name,
number and offset stay the SPL's, whatever the slot holds. The SPL picks a FIT configuration by
name (`Boot from fit configuration %s`) — which name it asks for, and so which configuration our
single-config FIT must answer (or its default), is measured with the first boot (TASK-0260 P3).

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
