---
title: TASK-0244 Board support v1a (host-first): `nexus-fdt` — the flattened device tree is the one hardware truth
status: Done 2026-09-22 (P0–P3: `nexus-fdt` + the board's own tree + both goldens host-tested; nxboot copies the tree into headroom, writes `/chosen/nexus,*` and hands the copy over; the kernel maps and parses it and prints what it read — `KSELFTEST: platform from fdt ok (… chosen.slot=a chosen.display=1280x800)` boot-proven, required in every profile. Recut 2026-09-22 from "Hardware Bring-up (RISC-V virt) v1.0a: DTB parser + SBI shim", Draft since 2025-12-29 with no code)
owner: @kernel-team @runtime
created: 2025-12-29
updated: 2026-09-22
depends-on: []
follow-up-tasks:
  - tasks/TASK-0245-bringup-rv-virt-v1_0b-os-kernel-uart-plic-timer-uartd-selftests.md
links:
  - Contract: docs/rfcs/RFC-0098-board-support-contract-fdt-truth-boot-chain.md (C1–C3, Phase 0)
  - Execution order: tasks/IMPLEMENTATION-ORDER.md (Block 1)
  - Measurement this is built on: docs/board/measurements/2026-09-22-stock-system/README.md
  - Board page: docs/board/bpi-f3.md
  - Consumers: source/boot/nxboot (a1 pass-through + `/chosen`), source/kernel/neuron/src/core/kmain.rs, source/init/nexus-init/src/bootstrap/helpers.rs (device discovery)
  - Playbook: CLAUDE.md
---

## History

Seeded 2025-12-29 as a QEMU-virt DTB parser + SBI shim with deterministic host tests; never
started. Recut 2026-09-22: the parser is the same idea, but its purpose is now the whole
platform — on QEMU and on the reference board — and the "SBI shim" half is dropped (the kernel
already talks SBI directly; Sstc/SBI timer selection is TASK-0245's).

## Context

Nothing in the tree parses a device tree; nxboot passes `a1` through opaquely and every
platform value is a QEMU-virt literal (RFC-0098 Context). The board's values were measured on
2026-09-22 from the running stock system: PLIC `riscv,plic0` at `0xe000_0000` (`riscv,ndev`
159, 16 contexts), UART `spacemit,pxa-uart` at `0xd401_7000`, `timebase-frequency` 24 MHz, 8
harts in a `cpu-map` of two clusters, `riscv,isa-extensions` listing `sstc`/`zicbom`/`svpbmt`,
two `memory@` banks (0–2 GiB, 4–6 GiB), a `reserved-memory` node, three `spacemit,k1-x-sdhci`
hosts, `spacemit,dpu-online2`/`spacemit,hdmi`, `snps,dwc3`, `img,rgx`, two GMACs — 153 SoC
nodes, of which Block 1 consumes about fifteen.

## Goal

`source/libs/nexus-fdt`: a `no_std`, `forbid(unsafe_code)`, allocation-free, bounded parser
of the flattened device tree (v17 structure block, strings block, memory reservation block)
with exactly the queries the consumers need:

- `memory_banks()` (every `/memory@*` `reg`), `reserved_ranges()` (`/reserved-memory` children
  + the mem-reserve block), `cpus()` (hart ids, `timebase-frequency`, `riscv,isa-extensions`,
  `mmu-type`, `cpu-map` cluster of each hart), `chosen()` (`stdout-path`, `bootargs`,
  `nexus,*`);
- `find_compatible(&[&str])` iterator yielding a node handle with `reg` (honouring
  `#address-cells`/`#size-cells` of the parent and `ranges` of `/soc`), `interrupts` +
  `interrupt-parent`, `clocks`/`clock-names`, `resets`, `power-domains`, `status`,
  `dma-coherent`, and arbitrary `prop(name)` bytes/u32/u64/string;
- `ChosenWriter`: in-place set of `/chosen/nexus,*` string/u64 properties using the headroom
  nxboot reserves (grows `totalsize`; refuses without headroom) — nxboot's only write path.

Host tests against two checked-in goldens: QEMU's dumped `virt` dtb (`-machine virt,dumpdtb=`)
and `config/board/bpi-f3/board.dtb` compiled by `dtc` from OUR `board.dts` (written from the
mainline SoC dts under its MIT option + the measured facts; never the vendor kernel's tree).
`test_reject_*`: truncated tree, bad magic, string offset past the block, `reg` shorter than
the cells, a `/chosen` write without headroom, cycles in `interrupt-parent`.

## Non-Goals

Overlays, `/aliases`-based lookups beyond `stdout-path` resolution, phandle-to-node maps
beyond what `interrupt-parent`/`clocks` need, a DTS writer, ACPI, runtime hot-plug.

## End state (binding)

- One crate, `source/libs/nexus-fdt` (≤ 600 LOC per file; `parse.rs`, `node.rs`, `props.rs`,
  `chosen.rs`), exported to nxboot and the kernel (both `no_std`), with the goldens under
  `source/libs/nexus-fdt/tests/goldens/` and `config/board/bpi-f3/board.dts` as their source.
- The kernel's platform (TASK-0245) and init's discovery consume only this crate; the grep
  gate `scripts/check-no-platform-literals.sh` (owned by TASK-0245) is what makes "one truth"
  enforceable.

## Packages

- **P0 Paper** — this recut; `board.dts` authored (MIT-derived, nodes: `/cpus` with cpu-map,
  `/memory@*`, `/reserved-memory`, `/chosen`, `plic`, `clint` (documentary), `uart0`, the three
  `sdh@`, `dpu`+`hdmi`, `dwc3`, `ehci`, `udc`, `gmac0/1`, `imggpu`, `clock-controller`,
  `reset-controller`, `power-controller`, `rtc`, PMIC i2c) with the measured `reg`/`interrupts`.
- **P1 Parser** — structure/strings walk, cells/ranges, the query API, goldens, `test_reject_*`.
- **P2 Chosen writer** — in-place `/chosen/nexus,*` with headroom; round-trip test.
- **P0–P2 built 2026-09-22.** `config/board/bpi-f3/board.dts` authored (root `bananapi,bpi-f3`,
  `spacemit,k1`; 8 harts + `cpu-map`, two memory banks, OpenSBI reserved, `/chosen` with
  `stdout-path = serial0`, PLIC with 16 `interrupts-extended` slots, clint (documentary), the
  syscon windows, uart0 with `reg-shift 2`, three SDHCI hosts (IRQ 99/100/101, eMMC 8-bit),
  DPU + HDMI, DWC3/EHCI/UDC, two GMACs, GPU `img,rgx`, RTC disabled), compiled with `dtc -p 512`
  into `tests/goldens/bpi-f3.dtb`; `tests/goldens/virt.dtb` dumped with the launcher's machine
  options. Crate: `header.rs` (bounds), `node.rs` (walk, props, `reg` with parent cells + one-level
  `ranges`, `interrupts` via `#interrupt-cells`, phandles, paths/aliases), `platform.rs` (banks,
  reserved, cpus/cpu-map/ISA, PLIC S-contexts, stdout, `/chosen` `nexus,*`), `chosen.rs` (in-place
  writer; `dtc -p` padding inside `totalsize` counts as headroom, strings block must stay last).
  Proof: 10 golden tests (timebase 10 vs 24 MHz, 4 vs 8 harts, cluster of hart 5 = 1, PLIC
  `0x0c00_0000`/`0xe000_0000` with S-contexts `(hart, 2·hart+1)` READ not derived, console via
  alias with `reg-shift`, virtio-mmio IRQ 1 at `0x1000_1000` as a property, eMMC IRQ 101, DPU
  139/138, GPU regs, chosen round-trip incl. remove+insert and same-length overwrite, refusal
  without headroom leaves the buffer byte-identical) + 8 `test_reject_*`; clippy `-D warnings`,
  pinned fmt, `riscv64imac-unknown-none-elf` build. 1 477 LOC, largest file 424.
- **P3 Consumers wired — Done 2026-09-22.** nxboot (`src/platform.rs`): validates the header in
  `a1` (magic, `totalsize` ≤ 1 MiB), copies the tree into a 2 KiB-headroomed buffer in its bump
  arena, writes `nexus,boot-slot`, `nexus,boot-record` (the handoff page address) and, from
  fw_cfg — read ONLY here now — `nexus,boot-profile` / `nexus,display-mode` when the lane passes
  them, reads the copy back through the parser (`nxboot: fdt ok (harts=… tb=…Hz slot=…)`) and
  jumps with `a1` = the copy; any refusal is `nxboot: PANIC (fdt: <reason>)` + SBI reset. Kernel:
  `start_rust(hartid, dtb)` keeps the firmware registers (`_start` touches only `sp`/`gp`),
  `boot_fdt::record` reads the header with paging off, `map_kernel_segments` identity-maps
  exactly the tree's pages read-only, `boot_fdt::report` parses after the space is active and
  prints `KSELFTEST: platform from fdt ok (uart=0x10000000 plic=0xc000000 tb=10000000Hz harts=1
  banks=1 boot_hart=0 chosen.slot=a chosen.profile=- chosen.display=1280x800)` — the `chosen.*`
  values prove the kernel reads the LOADER's copy (the fw_cfg knob came back through the tree);
  the FAIL twin carries the parser's reason. Markers registered (`bringup.toml`), REQUIRED in
  every profile by `scripts/qemu-test.sh` (`nxboot: fdt ok (` whenever the loader is in the
  chain). Boot-proven on `ci-os-smp1` three times while iterating; `just lint-kernel` clean.

## Constraints / invariants

- **No fake success**: the marker prints values READ from the tree, and the test compares them
  with the golden's known values.
- **Bounded parsing of trusted-but-checked input** (RFC-0098 Security): every offset/length
  checked; no `unwrap`/`expect`; a malformed tree is a typed error.
- No platform literal outside this crate's consumers (the gate lands with TASK-0245).

## Red flags / decision points

- **GREEN (measured 2026-09-22):** the board's tree is parseable by the same code as QEMU's
  (both v17; `#address-cells 2`, `#size-cells 2`; `ranges` empty on `/soc`).
- **YELLOW:** `/chosen` headroom — nxboot must reserve bytes at FIT build time
  (`scripts/build-fit.sh` pads the dtb); the writer refuses otherwise (never relocates).
- **GREEN:** license — `config/board/bpi-f3/board.dts` derives from the mainline SoC dts
  (`GPL-2.0 OR MIT`) under MIT; the vendor kernel's dts is reference only.

## Definition of Done

Host tests green on both goldens incl. every `test_reject_*`; QEMU profiles print the marker
with the dumped dtb's values; nxboot and the kernel have no other tree code; docs:
`docs/board/bpi-f3.md` links the dts, RFC-0098 Phase 0 ✅.
