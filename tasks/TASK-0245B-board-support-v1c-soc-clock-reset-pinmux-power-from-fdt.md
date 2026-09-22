---
title: TASK-0245B Board support v1c: `nexus-soc` + `socd` — clock gates, resets, pinmux and power domains from the FDT syscon nodes, ONE owner for every board driver
status: In Progress (P0 done 2026-09-22 — measured on the board, table with provenance, RFC-0106 seeded; was "seeded 2026-09-22 at Block 1 P0 as the B part of TASK-0245")
owner: @runtime @kernel-team
created: 2026-09-22
updated: 2026-09-22
depends-on:
  - tasks/TASK-0244-bringup-rv-virt-v1_0a-host-dtb-sbi-shim-deterministic.md
  - tasks/TASK-0245-bringup-rv-virt-v1_0b-os-kernel-uart-plic-timer-uartd-selftests.md
follow-up-tasks:
  - tasks/TASK-0246-* (SDHCI/eMMC: the first consumer)
  - tasks/TASK-0251-* (DPU/HDMI), tasks/TASK-0328-* (USB), tasks/TASK-0329-* (GPU), tasks/TASK-0248-* (GMAC)
links:
  - Contract: docs/rfcs/RFC-0106-soc-glue-one-owner-clocks-resets-power-pinmux.md (seeded at P0)
  - Board contract: docs/rfcs/RFC-0098-board-support-contract-fdt-truth-boot-chain.md (C3, Phase 1)
  - Execution order: tasks/IMPLEMENTATION-ORDER.md (Block 1 B1.3)
  - Measurement: docs/board/measurements/2026-09-22-stock-system/README.md ("R3 measured in detail": `clk_summary.txt`, `regmap-apmu.txt`, `regmap-mpmu.txt`, `regmap-ciu.txt`)
  - Grant model: docs/rfcs/RFC-0017-device-mmio-access-model-v1.md (new class `device.mmio.syscon`)
  - Playbook: CLAUDE.md
---

## History

Seeded 2026-09-22 as the B part of TASK-0245 ("nexus-soc, a library every board driver links").
P0 2026-09-22: measured the board and read the mainline documentation; the library alone is
not the end state — the APMU packs several devices' gates and resets into shared registers
(the USB register serves the EHCI, the OTG controller and the DWC3; the SDH0 register holds the
AXI gate all three SD hosts share), so a read-modify-write from two services is a race. The end
state is ONE owner (`socd`) with the library as its host-tested core (RFC-0106).

## Context (measured 2026-09-22)

**Consumers in the live tree** (vendor ids; the registers are the same as mainline's):

| node | clocks (`clock-names`) | resets (`reset-names`) | power-domain | pinctrl |
|---|---|---|---|---|
| `sdh@d4280000` SD | `sdh-io`, `sdh-core`, `aib-clk` | `sdh_axi`, `sdh0` | 0 (BUS) | `default`, `fast` |
| `sdh@d4280800` SDIO/WiFi | `sdh-io`, `sdh-core` | `sdh_axi`, `sdh1` | 0 | yes |
| `sdh@d4281000` eMMC | `sdh-io`, `sdh-core` | `sdh_axi`, `sdh2` | 0 | none |
| `udc@c0900100` | one | one | — | — |
| `ethernet@cac8{0,1}000` | `emac-clk`, `ptp-clk` | `emac-reset` | 0 | yes |
| `hdmi@C0400500` / dpu | `hmclk`; `dpu_mclk`/`hclk`/`esc`/`bit`/`px` | `hdmi_reset`; dpu resets | 7 (HDMI) | HDMI DDC |
| `imggpu@cac00000` | `gpu_clk` | one | 2 (GPU) | — |
| `usbdrd3` / ehci / phys | `usb30_clk`, `usb_axi_clk`, `usb_p1_aclk` | ahb/vcc/phy, axi, p1_axi | — | — |

**Register truth** (offsets inside the windows our tree names; bit semantics from the mainline
driver DOCUMENTATION — GPL code is reference only, the facts are transcribed; values = the
stock system's state, `regmap-apmu.txt`):

| window / register | offset | gate (set = on) | reset (APMU: set = released) | mux / div / FC | stock value |
|---|---|---|---|---|---|
| APMU `SDH0_CLK_RES_CTRL` | 0x054 | sdh_axi BIT3, sdh0 BIT4 | sdh_axi BIT0, sdh0 BIT1 | mux 8..10, div 5..7, FC BIT11 | `0x0000411b` |
| APMU `SDH1_CLK_RES_CTRL` | 0x058 | sdh1 BIT4 | sdh1 BIT1 | mux 8..10, div 5..7, FC BIT11 | `0x00000052` |
| APMU `SDH2_CLK_RES_CTRL` | 0x0e0 | sdh2 BIT4 | sdh2 BIT1 | mux 8..10 (0 = pll1_d6 409.6, 1 = pll1_d4 614.4, 2 = pll2_d8 375, 3 = pll1_d3 819.2, 4 = pll1_d11 223.4, 5 = pll1_d13 189, 6 = pll1_d23 106.8 MHz), div 5..7, FC BIT11 | `0x00000052` |
| APMU `USB_CLK_RES_CTRL` | 0x05c | usb_axi BIT1, usb_p1 BIT5, usb30 BIT8 | usb_axi BIT0, usbp1_axi BIT4, usb30 ahb BIT9 / vcc BIT10 / phy BIT11 | — | `0x00000f33` |
| APMU `LCD_CLK_RES_CTRL1` | 0x044 | dpu_hclk BIT5, dpu_esc BIT2, dpu_bit BIT16 | dpu_esc BIT3, dpu_hclk BIT4, mipi BIT15, v2d BIT27 | esc mux 0..1; bit mux 20..22 div 17..19 FC BIT31; px div 21..23 FC BIT30 | `0x08805180` |
| APMU `LCD_CLK_RES_CTRL2` | 0x04c | dpu_mclk BIT0 (gate in CTRL1), px gate BIT16 (CTRL1) | dpu_mclk BIT9 | mclk mux 5..7 div 1..4 FC BIT29; px mux 17..20 | `0x01040104` |
| APMU `HDMI_CLK_RES_CTRL` | 0x1b8 | hmclk BIT0 | hdmi BIT9 | mux 5..7 (pll1_d6/d5/d4/d8), div 1..4, FC BIT29 | `0x01040321` |
| APMU `GPU_CLK_RES_CTRL` | 0x0cc | gpu BIT4 | gpu BIT1 | mux 18..20, div 12..14, FC BIT15 | `0x00000012` |
| APMU `EMAC{0,1}_CLK_RES_CTRL` | 0x3e4 / 0x3ec | bus BIT0, ptp BIT15 | emac BIT1 | — | `0x0000a007` |
| APMU `ACLK_CLK_CTRL` (pmua_aclk) | 0x388 | — | — | mux 0, div 1..2, FC BIT4 | `0x00000001` |
| APBC `UART1_CLK_RST` (= uart0) | 0x00 | core BIT1, bus BIT0 | **assert = set BIT2** | mux 4..6 | on (console) |
| APBC `AIB_CLK_RST` (pinctrl) | 0x3c | BIT1 / BIT0 | assert BIT2 | — | on |
| MPMU `POSR` | 0x010 | PLL1/2/3 lock BIT27..29 | — | — | `0x3bb83c00` (all locked) |
| pinctrl pad `N` | `N * 4` | — | — | mux GENMASK(2,0), strong-pull BIT3, slew-en BIT7, schmitt 8..9, drive 10..12, pulldown BIT13, pullup BIT14, pull-en BIT15; IO power domain block at +0x800 (MMC `+0x1c`, V18EN BIT2) | — |

FC (frequency change): after writing mux/div, set the FC bit and poll until the hardware clears
it (bounded). Rates: the PLL post-dividers are fixed-ratio children of PLL1 2457.6 MHz, PLL2
3000 MHz, PLL3 3200 MHz (locked, `POSR`); `sdh2_clk` runs at 375 MHz from `pll2_d8` (mux 2) on
the stock system. Pin groups (mainline `k1-pinctrl.dtsi`, GPL-2.0 OR MIT): `mmc1` pads
104..109 func 0 (pull-up, drive 19, 3.3 V; `uhs` = drive 42, 1.8 V), `gmac0` pads 0..12 func 1
+ clk_ref pad 45, `gmac1` pads 29..41 + 46, `uart0` pads 68/69 func 2. Power domains: mainline
has NO driver for the K1 power controller; the vendor node is two syscon wrappers (mpmu/apmu);
domain 0 (BUS) is always on — every Block-1 consumer lives there. Domains 2 (GPU) and 7 (HDMI)
need a measurement first (P3: regmap diff of APMU/MPMU across a domain toggle on the stock
system).

## Goal

One owner for the SoC glue: `socd`, a driver-kit service that holds the syscon and pinctrl
windows the tree lists (`device.mmio.syscon`, granted by compatible) and brings a consumer
node up on request — power domain on, resets released, clocks gated on (mux/div/FC as the
node's rate demands), pads muxed — with a read-back after every step and a bounded poll for
every self-clearing bit; `nexus-soc`, the `no_std`/`forbid(unsafe_code)` library it is built
from: the provider model, the K1 tables (mainline ids from the BSD-licensed binding header as
the vocabulary, register offsets and bits with provenance), the operations over
`nexus_hal::Bus`, host-tested against a register file seeded from the measured stock state.
Our tree binds consumers the standard way (`clocks`, `resets`, `power-domains`, `pinctrl-0`);
on QEMU virt the tree has no providers and `socd` answers "nothing to do" — the FDT decides.

## Non-Goals

Dynamic frequency scaling, cpufreq, thermal, PLL programming beyond reading what the SPL set,
audio/camera/video domains, suspend/resume, a userspace clock framework API beyond "bring this
node up / this clock's rate".

## End state (binding)

- `source/libs/nexus-soc`: `Provider` (by compatible: `spacemit,k1-syscon-{apbc,apmu,mpmu,apbc2}`,
  `spacemit,k1-pll`, `spacemit,k1-pinctrl`), `Table` entries `{ id, kind: Gate|MuxDivGate|Reset|Pad|
  Domain, reg, fields }` with a provenance line each, `Glue::bring_up(node) -> Steps` executed over a
  `Bus` with read-back, `rate(node, name)`, pinmux from `pinmux = <(pin << 16) | func>` +
  pinconf; `MockBus` host tests seeded from `regmap-apmu.txt`: "eMMC from the stock state = no
  write" and "eMMC from a cold file = exactly these bits".
- `nexus-fdt`: phandle-specifier lists (`clocks`, `resets`, `power-domains`, `pinctrl-0`) resolved
  to `(provider node, cells)` through `#clock-cells`/`#reset-cells`/`#power-domain-cells`.
- `config/board/bpi-f3/board.dts`: providers carry the cells properties; every consumer names its
  clocks/resets/domains/pins with mainline ids (`config/board/include/dt-bindings/…`, BSD-2-Clause
  copies with their notice); pin groups for mmc1, gmac0/1, uart0 (MIT).
- `source/drivers/soc/socd`: driver-kit service; init grants every provider window by compatible
  (`device.mmio.syscon`, policyd deny-by-default, one holder); protocol `soc` v1 (nexus-wire):
  `OP_BRING_UP(node_path) -> {status, steps_done}`, `OP_CLOCK_RATE(node_path, name) -> hz`;
  requester = `sender_service_id`, allowed per class `soc.glue.<class>` (policyd rule per consumer
  service); markers `socd: ready (providers=N)` / `socd: ready (no soc glue in this tree)`,
  `socd: bring-up <node> ok (domain=… resets=… clocks=… pads=…)`.
- Consumers call `nexus_soc::client::bring_up(node_path)` before touching their registers; a
  driver crate never maps a syscon window (the literal gate carries the six window addresses).

## Packages

- **P0 — Paper + measurement (done 2026-09-22).** The tables above; RFC-0106 seeded; the
  measurement files; the decision for one owner.
- **P1 — Library + tree.** `nexus-soc` with the K1 tables for SDH/UART/AIB/USB/EMAC/HDMI/DPU/GPU
  clocks and resets and the pad format; `nexus-fdt` specifier resolution; `board.dts` bindings +
  binding headers + pin groups; goldens rebuilt; host tests incl. the measured-state tests.
- **P2 — `socd` + protocol + policy + init.** Service, `soc` wire protocol, policy class and
  per-consumer rules, init spawn + grants by compatible, client API; QEMU proof
  `socd: ready (no soc glue in this tree)` in every profile and a selftest probe that
  `bring_up` of a node without providers answers `NotNeeded`; the first real consumer is
  TASK-0246's SDHCI on the board (`socd: bring-up sdh@d4281000 ok`).
- **P3 — Power domains + the display/USB/GPU sets.** Measurement recipe on the stock system,
  domains 2/7, HDMI DDC and USB pads; consumed by TASK-0251/0328/0329.

## Constraints / invariants

- A driver never touches a syscon window; `socd` is the only writer (RFC-0106).
- Every enable is followed by a read-back or status poll with a bounded timeout; a self-clearing
  bit that does not clear is a loud failure with the register value in the marker.
- No window address in any table: addresses come from the tree's `reg`; tables hold offsets.

## Red flags / decision points

- **RED (P3):** the power controller's registers are undocumented in mainline; a wrong write can
  hang the SoC. Measure (regmap diff) before the first write; GPU/HDMI stay in the stock state
  until then (they are on).
- **YELLOW:** the FC bit protocol (set, poll clear) is documented only in the driver; the first
  mux change on the board is done on SDH2 where the stock value is known and reversible.

## Definition of Done

P1–P2 green on QEMU (host tests, `socd` markers in every profile, the probe); the SDHCI consumer
up on the board through `socd` (TASK-0246); the literal gate carries the window addresses; docs
(RFC-0106 Phase 1 ✅, `docs/architecture/06-boot-and-bringup.md` glue paragraph, CHANGELOG).
