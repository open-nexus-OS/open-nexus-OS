---
title: TASK-0245B Board support v1c: `nexus-soc` — clock gates, resets, pinmux and power domains from the FDT syscon nodes, for every board driver
status: Draft (seeded 2026-09-22 at Block 1 P0 as the B part of TASK-0245)
owner: @runtime @kernel-team
created: 2026-09-22
depends-on:
  - tasks/TASK-0244-bringup-rv-virt-v1_0a-host-dtb-sbi-shim-deterministic.md
  - tasks/TASK-0245-bringup-rv-virt-v1_0b-os-kernel-uart-plic-timer-uartd-selftests.md
follow-up-tasks: []
links:
  - Contract: docs/rfcs/RFC-0098-board-support-contract-fdt-truth-boot-chain.md (C3, Phase 1)
  - Execution order: tasks/IMPLEMENTATION-ORDER.md (Block 1 B1.3)
  - Measurement: docs/board/measurements/2026-09-22-stock-system/README.md ("Clocks / resets / power", `clk_summary.txt`)
  - Grant model: docs/rfcs/RFC-0017-device-mmio-access-model-v1.md (new class `device.mmio.syscon`)
  - Playbook: CLAUDE.md
---

## Context (measured 2026-09-22)

Every board device (SDHCI, DPU/HDMI, DWC3/xHCI, GMAC, GPU) names its clocks, resets and power
domain in the FDT and expects them enabled before its registers answer. The SoC has ONE clock
controller (`spacemit,k1x-clock`) and ONE reset controller (`spacemit,k1x-reset`), both a set
of syscon windows: `mpmu 0xd4050000/0x209c`, `apmu 0xd4282800/0x400`, `apbc 0xd4015000/0x1000`,
`apbs 0xd4090000/0x1000`, `ciu 0xd4282c00/0x400`, `dciu`, `ddrc 0xc0000000`, `apbc2`, `rcpu`,
`rcpu2`, `audpmu`; inputs `vctcxo_24` (24 MHz), `vctcxo_3`, `vctcxo_1`, `pll1_2457p6_vco`,
`clk_32k`; a `spacemit,power-controller` with 9 domains (BUS 0, VPU 1, GPU 2, …, HDMI 7);
`pinconf-single-aib` pinctrl at `0xd401e000`. The stock kernel runs with `clk_ignore_unused` —
it never gates what the SPL enabled, and `clk_summary.txt` shows uart/emac/pcie/dma/usb clocks
on at boot. R3 ("what the SPL leaves on") is therefore partly answered: enough for the console
and USB; the SDHCI, display and GPU gates must be driven by us.

## Goal

`source/libs/nexus-soc`: a `no_std`, `forbid(unsafe_code)` (over `nexus_hal::Bus`) library
that turns an FDT node's `clocks`/`clock-names`, `resets`/`reset-names`, `power-domains` and
`pinctrl-0` into register writes on the syscon windows — gate on/off, rate read-back for the
few dividers we need (sdh-core, hmclk, gpu_clk), reset assert/deassert, power-domain on with
its status poll, pinmux for the functions we use (SDHCI bus, HDMI DDC, GMAC RGMII). Register
semantics come from the mainline SoC drivers' documentation and the dts (the vendor kernel's
code is reference only). Granted to drivers as the policy class `device.mmio.syscon`
(deny-by-default, one grant per consumer service).

## Non-Goals

Dynamic frequency scaling, cpufreq, thermal, PLL programming beyond reading what the SPL set,
audio/camera/video domains, suspend/resume.

## Packages

- **P0** — table of every clock/reset/domain/pin the Block-1 and Block-2 drivers need, with
  register offsets, from the mainline dts + docs; host tests over a mock `Bus`.
- **P1** — gate/reset/power API + the SDHCI set (the first consumer, TASK-0246); board marker
  `soc: clk sdhci2 on` on the serial console.
- **P2** — DPU/HDMI and USB sets (TASK-0251, TASK-0328), GMAC and GPU sets added by their
  ledgers through the same table.

## Constraints / invariants

- A driver never touches a syscon window itself; `nexus-soc` is the only writer, and it is
  granted per service (no shared mutable syscon across services without an owner — if two
  services need the same window, ONE syscon service owns it; decided at P0 by the table).
- Every enable is followed by a read-back or status poll with a bounded timeout; no "assumed
  on".

## Definition of Done

The table with provenance; host tests; the first consumer (SDHCI) up on the board through
this library; the gate that no driver crate contains a syscon address.
