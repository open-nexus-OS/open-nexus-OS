# RFC-0106: SoC glue — one owner for clocks, resets, power domains and pinmux (`socd` + `nexus-soc`)

- Status: Draft (seeded 2026-09-22 at TASK-0245B P0)
- Owners: @runtime @kernel-team
- Created: 2026-09-22
- Last Updated: 2026-09-22
- Links:
  - Tasks: `tasks/TASK-0245B-board-support-v1c-soc-clock-reset-pinmux-power-from-fdt.md` (execution + proof); consumers `tasks/TASK-0246-*` (SDHCI), `TASK-0251-*` (DPU/HDMI), `TASK-0328-*` (USB), `TASK-0329-*` (GPU), `TASK-0248-*` (GMAC)
  - Related RFCs: `docs/rfcs/RFC-0098-board-support-contract-fdt-truth-boot-chain.md` (C3: the tree is the truth; this RFC is its SoC-glue arm), `docs/rfcs/RFC-0017-device-mmio-access-model-v1.md` (grant model; new class `device.mmio.syscon`), `docs/rfcs/RFC-0093-*` (service topology / declared slots)
  - Measurement: `docs/board/measurements/2026-09-22-stock-system/README.md` ("R3 measured in detail")

## Status at a Glance

- **Phase 0 (paper + measurement, the table with provenance)**: ✅ 2026-09-22 — TASK-0245B P0
- **Phase 1 (`nexus-soc` library, tree bindings, specifier resolution)**: ✅ 2026-09-22 — TASK-0245B P1 (host-proven against the measured APMU state)
- **Phase 2 (`socd`, protocol, policy class, init grants, first consumer)**: ⬜ — TASK-0245B P2 + TASK-0246
- **Phase 3 (power domains, display/USB/GPU sets)**: ⬜ — TASK-0245B P3 with TASK-0251/0328/0329

Definition: "Complete" = the contract below is implemented and the proof gates are green on
QEMU and on the board.

## Scope boundaries (anti-drift)

- **This RFC owns**: who may write a syscon/pinctrl register (exactly one service), how a
  consumer asks for its node to be brought up, how the tree binds consumers to providers, and
  the invariants every step obeys (read-back, bounded polls, no assumed state).
- **This RFC does NOT own**: register values themselves (the tables live in `nexus-soc` with a
  provenance line each), device drivers, PLL programming, DVFS, thermal, suspend.

### Relationship to tasks

TASK-0245B carries the packages and proof commands; consumer tasks carry their own bring-up
markers through `socd`.

## Context

Every board device names clocks, resets, a power domain and pads in the tree and expects them
live before its registers answer. Measured 2026-09-22 on the reference board: the SoC packs
several devices' gate and reset bits into SHARED registers — the APMU USB register serves the
EHCI, the OTG controller and the DWC3; the SDH0 register holds the AXI gate all three SD hosts
share; the LCD registers serve the display controller's five clocks. A library each driver
links would make two services read-modify-write the same register: a race no lock can span
process boundaries. The stock kernel side-steps the question with a single clock framework in
one address space; we have services.

## Goals

- ONE writer for every syscon and pinctrl window: `socd`, a driver-kit service.
- Consumers stay ignorant of registers: they ask for their node, by path, and get a verdict.
- The tree binds consumers to providers the standard way (`clocks`, `resets`, `power-domains`,
  `pinctrl-0` with `#…-cells`), so a node is self-describing and QEMU virt (no providers) needs
  no cfg: `socd` answers "nothing to do".
- Every step is read back; every self-clearing bit is polled with a bound; nothing is assumed on.

## Non-Goals

DVFS/cpufreq, thermal, PLL programming, audio/camera/video domains, suspend/resume, a general
clock-framework API (rates are read, not set, beyond what a node's binding demands).

## Constraints / invariants (hard requirements)

- **Determinism**: markers name the node and the steps; a failure names the register and value.
- **No fake success**: `bring-up … ok` only after the read-back of every step.
- **Bounded resources**: one request in flight per consumer; polls bounded (FC bit, domain
  status); table size fixed at build time.
- **Security floor**: identity = `sender_service_id`; a consumer may bring up only node classes
  policyd allows it (`soc.glue.<class>`, deny-by-default); `socd` alone holds
  `device.mmio.syscon`; no consumer holds a syscon window (gate: the six window addresses are
  platform literals).
- **Stubs policy**: on a tree without providers `socd` reports `no soc glue in this tree` and
  answers `NotNeeded` — an honest verdict, never `ok`.

## Proposed design

### Contract / interface (normative)

- **Providers** are the tree nodes with `#clock-cells`, `#reset-cells`, `#power-domain-cells`
  or a pinctrl compatible; init grants each provider window to `socd` by compatible
  (`device.mmio.syscon`); `socd` builds one `Provider` per window from the `nexus-soc` table
  keyed by compatible.
- **Consumers** carry `clocks`/`clock-names`, `resets`/`reset-names`, `power-domains`,
  `pinctrl-0`/`pinctrl-names` (mainline ids; the binding headers are BSD-2-Clause copies).
- **Protocol `soc` v1** (nexus-wire, request/reply on `socd`'s server endpoint):
  - `OP_BRING_UP { node_path }` → `{ status, domain_on, resets_released, clocks_on, pads_set }`;
    order: power domain → resets deassert → clocks enable (mux/div per binding, FC poll) →
    pads; `status ∈ { Ok, NotNeeded, Denied, NoSuchNode, Failed{step, reg, value} }`.
  - `OP_CLOCK_RATE { node_path, clock_name }` → `{ hz }` computed from the table's parent tree
    and the mux/div fields read back; PLL rates from the locked PLL descriptors.
- **Client** `nexus_soc::client::bring_up(node_path)` / `clock_rate(node_path, name)`; a driver
  calls `bring_up` before its first register access and treats `NotNeeded` as success.
- **Markers**: `socd: ready (providers=N)` | `socd: ready (no soc glue in this tree)`;
  `socd: bring-up <node> ok (domain=… resets=… clocks=… pads=…)`; `socd: bring-up <node> FAIL
  (step=… reg=0x… val=0x…)`; selftest `SELFTEST: soc glue not needed ok` on QEMU.

### Register semantics (facts transcribed from the mainline documentation and measured)

Gates: set the bit to enable. APMU resets: the bit SET means released (deassert = set).
APBC/APBC2 resets: bit 2 SET asserts. Mux/div changes: write the fields, set the
frequency-change bit, poll until it clears. Pads: one 32-bit register per pad at `pad * 4`
(mux bits 0..2, pull/drive/schmitt/slew fields). Power domains: undocumented in mainline —
Phase 3 measures before the first write. The complete table with offsets, bits and the
stock values is TASK-0245B's "Register truth".

## Alternatives considered

- **A library every driver links** (the P0 seed): rejected at P0 — shared registers across
  services.
- **Clock control in the kernel**: rejected — drivers and policy stay out of the kernel
  (CLAUDE.md architecture boundary); the kernel's own needs (console, timer) are already on
  when it starts.
- **`socd` as part of `blkd` or `gpud`**: rejected — the USB, GPU and display consumers are
  different services; the owner must serve all of them.

## Proof plan

- Host: `nexus-soc` tests over a mock register file seeded from `regmap-apmu.txt` (the
  measured state): the eMMC bring-up from the stock state writes nothing; from a cold file it
  writes exactly the documented bits; a stuck FC bit fails loudly.
- QEMU: `socd: ready (no soc glue in this tree)` + `SELFTEST: soc glue not needed ok` in every
  profile.
- Board: `socd: bring-up sdh@d4281000 ok (…)` on the serial console before the eMMC answers
  (TASK-0246), then the display/USB/GPU sets with their tasks.
