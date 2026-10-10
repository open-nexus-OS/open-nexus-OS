# RFC-0106: SoC glue — one owner for clocks, resets, power domains and pinmux (`socd` + `nexus-soc`)

- Status: In Progress (Phases 0–1 ✅; Phases 2–3 proven on the board for storage, display and USB — TASK-0245B Done 2026-10-10; the GPU set and the per-class floor land with G1, TASK-0329; seeded 2026-09-22 at TASK-0245B P0)
- Owners: @runtime @kernel-team
- Created: 2026-09-22
- Last Updated: 2026-10-10 (Block 1 closure: the USB set ✅ on the board; open items handed to G1 — TASK-0329)
- Links:
  - Tasks: `tasks/TASK-0245B-board-support-v1c-soc-clock-reset-pinmux-power-from-fdt.md` (execution + proof); consumers `tasks/TASK-0246-*` (SDHCI), `TASK-0251-*` (DPU/HDMI), `TASK-0328-*` (USB), `TASK-0329-*` (GPU), `TASK-0248-*` (GMAC)
  - Related RFCs: `docs/rfcs/RFC-0098-board-support-contract-fdt-truth-boot-chain.md` (C3: the tree is the truth; this RFC is its SoC-glue arm), `docs/rfcs/RFC-0017-device-mmio-access-model-v1.md` (grant model; new class `device.mmio.syscon`), `docs/rfcs/RFC-0093-*` (service topology / declared slots)
  - Measurement: `docs/board/measurements/2026-09-22-stock-system/README.md` ("R3 measured in detail")

## Status at a Glance

- **Phase 0 (paper + measurement, the table with provenance)**: ✅ 2026-09-22 — TASK-0245B P0
- **Phase 1 (`nexus-soc` library, tree bindings, specifier resolution)**: ✅ 2026-09-22 — TASK-0245B P1 (host-proven against the measured APMU state)
- **Phase 2 (`socd`, protocol, policy class, init grants, first consumer)**: 🟨 — socd + protocol + policy + init grants ✅ 2026-09-22 (TASK-0245B P2, QEMU: `socd: ready (no soc glue in this tree)`, `SELFTEST: soc glue not needed ok` in every profile); TASK-0246 P4c (2026-09-25): socd runs in the core plane on declared slots (no route ask — it serves before init's responder does), the block owner asks it through the shared client `nexus_ipc::socd` to bring the disk's node up before touching the controller and, on the K1, for the `io` clock's rate (QEMU: `blkd: backend ok (… soc=not-needed …)` in every boot); the board's eMMC is the first `ok` (TASK-0246 P6). TASK-0246B P2 (2026-09-26): the two node operations live in `nexus-soc` once — `bring_up` (plan, then execute; an empty plan is `NotNeeded`) and `clock_rate` — and `socd` answers with them; the loader runs them for its boot disk before any service exists (the loader clause below). Open: the per-class floor below (`soc.glue.<class>`) is still the one `soc.glue` capability — it lands with the next new socd consumer, G1 (TASK-0329, "Input from Block 1")
- **Phase 3 (power domains, display/USB/GPU sets)**: 🟨 — display set ✅ 2026-09-30 on the board (TASK-0245B P3): power domain 7 hardware-sequenced from the stock system's own tree (`docs/board/measurements/2026-09-30-power-domains/`), `hmclk` at the rate the node demands (`assigned-clock-rates`), the display node trimmed to what the pipeline uses, socd's marker with every touched register's words — one board cycle decided the protocol (`0x3f4 0x0>0x11`, `0x0f0` bit 15, `0x1b8` to the stock word), `SELFTEST: soc glue display ok` is a `board-headless` rung; pads ✅ 2026-10-03 on the board for the display set (the encoder's four, the K1 pad map measured against the stock system, the live words reproduced; drive-strength tables still to measure); the USB set ✅ 2026-10-06 on the board (TASK-0328 U3: the controller, both PHYs and the hub's power pins through socd, `[PASS] board-visible`); the GPU set with G1 (TASK-0329)

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
    order: power domain → resets deassert → clocks enable → the rates the node demands
    (the standard `assigned-clocks` / `assigned-clock-rates`: met exactly by one parent and
    one divider or refused, FC poll; after the gates, so the switch happens on a running
    clock of an idle consumer) → pads; `status ∈ { Ok, NotNeeded, Denied, NoSuchNode,
    Failed{step, reg, value} }`.
  - `OP_CLOCK_RATE { node_path, clock_name }` → `{ hz }` computed from the table's parent tree
    and the mux/div fields read back; PLL rates from the locked PLL descriptors.
- **Client** `nexus_soc::client::bring_up(node_path)` / `clock_rate(node_path, name)`; a driver
  calls `bring_up` before its first register access and treats `NotNeeded` as success.
- **Markers**: `socd: ready (providers=N)` | `socd: ready (no soc glue in this tree)`;
  `socd: bring-up <node> ok (domains=… resets=… clocks=… rates=… pads=… gpios=… writes=…)`;
  `socd: bring-up <node> FAIL (step=read-back|frequency-change|domain|range|settle reg=<window>+0x…
  val=0x…)`, `FAIL (refused: <why>)`, `FAIL (denied)`, `FAIL (no such node)`; both `ok` and
  `FAIL (step=…)` end with every register the bring-up touched as `<window>+<offset>:<before>>
  <after>`, all hex (one console line; words that do not fit are counted, never cut); selftest
  `SELFTEST: soc glue not needed ok` on QEMU.
- **Pads** (Phase 3, 2026-10-03): `pinctrl-0` of the node — every pin of every group, placed by
  the provider's pad map (the K1's measured against the stock system's `gpio-ranges` and live
  pad words; an unplaced pin is refused) — sets the fields the stock words confirm (function,
  pull and its direction, strong pull clear, edge detection cleared); drive strength and
  schmitt stay as found until their tables are measured per IO domain.
- **Glue words** (amendment 2026-10-05, TASK-0328 U3): a consumer may name provider words
  its vendor driver writes as one value — `nexus,glue-words = <&provider offset mask value>`
  (the provider declares `#nexus,glue-cells = <3>`): the `mask` bits set to `value` after the
  clocks run, read back, the rest of the word left as found; bits already so write nothing.
  The mask exists because a word mixes glue with status: the USB host's APMU word at 0x3c8 is
  `0x0b008000` on the stock system, but its bits 24..25 ignore a write (board cycle 1,
  `docs/board/measurements/2026-10-05-usb-cycle1/`) — the glue is bit 15. A provider without
  the cells, or a mask of 0, refuses the node.
- **GPIO lines and settles** (amendment 2026-10-05, TASK-0328 U3): the GPIO block is a
  provider kind (`spacemit,k1-gpio`, granted like the syscons into `SYSCON_MMIO_SLOTS`), and a
  node's supply and reset lines are glue — the on-board USB hub's binding (the stock
  `usb3hub@0` shape: `hub-gpios`, `vbus-gpios`, `vbus-delay-ms`, `<&gpio bank line flags>`):
  after the pads, each hub line is made an output and driven (high; low for an active-low
  flag) through the bank's masked set/clear registers — never a read-modify-write of another
  writer's lines (the loader and the kernel drive the LED on the same bank) — and read back
  from the bank's direction and level words (the level word follows the pin: it is read again,
  a bounded number of times, before a miss is a fault — board cycle 5); then the start-up
  delay (`vbus-delay-ms`, at most
  1000) is spent on `socd`'s declared kernel one-shot (a settle, never a spin), then VBUS. A
  bank the block does not have, a line past 32 or a longer delay is refused before any bus
  access; without the timer a plan with a settle fails as `step=settle`. The marker gains
  `gpios=`. The loader never drives a supply.
- **The node operations** (`nexus_soc::bring_up`, `nexus_soc::clock_rate`; TASK-0246B P2) are
  the one definition of "up" and of "the rate": `socd` answers `OP_BRING_UP` and
  `OP_CLOCK_RATE` with them, and nothing else re-derives them.
- **The loader** (TASK-0246B P2): before any service exists, nxboot runs the same node
  operations over the same tables for the one node it is about to boot from — the provider
  windows at the physical addresses the tree names — and reads that node's `io` clock. It is a
  single program on one hart that ends before `socd` starts, so no register ever has two
  writers at once; while the OS runs, `socd` stays the one writer. The loader's bring-up leaves
  the node as `socd`'s would, so `socd`'s later bring-up of the same node writes nothing (every
  step reads back first). A glue fault on the loader's side is a named skip of that disk with
  the register and the value it read, never a boot from a host whose glue is not up.

### Register semantics (facts transcribed from the mainline documentation and measured)

Gates: set the bit to enable. APMU resets: the bit SET means released (deassert = set) —
except PCIe port A's global reset (`0x3cc` bit 8, shared by the SuperSpeed USB PHY), whose
bit SET asserts (the vendor reset table's one inverted entry; the stock word `0x480` runs it
clear, our loader left it set — TASK-0328 U3, board cycle 10). The table carries the polarity
per entry; a tree names the reset, never the bit. APBC/APBC2 resets: bit 2 SET asserts. Mux/div changes: write the fields, set the
frequency-change bit, poll until it clears. Pads: one 32-bit register per pad at `pad * 4`
(mux bits 0..2, pull/drive/schmitt/slew fields). Power domains: undocumented in mainline;
the stock system's own tree describes each domain's control word (mode, request, two sleep
bits, isolation) and its status bits, the live APMU names the status word (Phase 3,
`docs/board/measurements/2026-09-30-power-domains/`). A hardware-sequenced domain comes up
when its mode bit is set with the request low and the request is then raised; its status bit
is polled with a bound; a domain already on is left alone. A software-sequenced domain (the
driver walks isolation and the sleep bits) is refused until its consumer measures it. The
complete table with offsets, bits and the stock values is TASK-0245B's "Register truth".

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
