---
title: TASK-0245 Board support v1b (OS): the kernel's platform comes from the FDT — UART, PLIC, timer, memory ranges, hart list; kernel + nxboot position-independent; init discovers devices from the FDT
status: Done (2026-09-22 — P1–P4 + DoD sweep; QEMU proof complete, the board's serial proof follows with TASK-0327B; recut 2026-09-22 to the end state — `hal/platform.rs` replaces `hal/virt.rs`: console/PLIC/timer/timebase from the tree, Sstc chosen by the ISA list, no CLINT; smp1 green; recut 2026-09-22 to the end state — Block 1 B1.2 of the hardware fast track; was "Hardware Bring-up (RISC-V virt) v1.0b: kernel UART/PLIC/timer + userspace uartd + selftests", Draft since 2025-12-29)
owner: @kernel-team
created: 2025-12-29
updated: 2026-09-22
depends-on:
  - tasks/TASK-0244-bringup-rv-virt-v1_0a-host-dtb-sbi-shim-deterministic.md
follow-up-tasks:
  - tasks/TASK-0245B-board-support-v1c-soc-clock-reset-pinmux-power-from-fdt.md
  - tasks/TASK-0286-kernel-memory-accounting-v1-rss-pressure-snapshots.md
links:
  - Contract: docs/rfcs/RFC-0098-board-support-contract-fdt-truth-boot-chain.md (C2, C3, Phase 1)
  - Execution order: tasks/IMPLEMENTATION-ORDER.md (Block 1)
  - Measurement: docs/board/measurements/2026-09-22-stock-system/README.md
  - Hardcodes this deletes: source/kernel/neuron/src/hal/virt.rs, hal/plic.rs, arch/riscv/mod.rs (CLINT), diag/uart.rs, mm/kernel_layout.rs (UART/PLIC windows), core/kmain.rs (fw_cfg), source/init/nexus-init/src/bootstrap/helpers.rs:107-113 (virtio window), bootstrap/route_provision.rs:220 (RTC)
  - Display-mode syscall this prepares for deletion: docs/rfcs/RFC-0074-display-mode-authority-fwcfg.md (deleted in TASK-0251)
  - Playbook: CLAUDE.md
---

## History

Seeded 2025-12-29 as "kernel UART/PLIC/timer for virt + a userspace uartd"; never started.
Recut 2026-09-22: the same three subsystems, but their values come from the FDT, the kernel
becomes position-independent, and `uartd` is dropped — the kernel-owned console
(`debug_write`) is the end state, a userspace UART service would be a second console.

## Context (measured 2026-09-22)

| Value | QEMU virt (hardcoded today) | Board (from the FDT) |
|---|---|---|
| UART | `0x1000_0000`, `ns16550a`, 1-byte stride | `0xd401_7000`, `spacemit,pxa-uart` (`intel,xscale-uart` in mainline), 4-byte stride, clock `slow_uart` |
| PLIC | `0x0c00_0000`, 53 sources, contexts by hart | `0xe000_0000` (64 MiB), `riscv,ndev 159`, 16 contexts (`interrupts-extended` M=11/S=9 per hart) |
| timer | CLINT MMIO `0x0200_0000` + `mtimecmp` (M-mode registers touched from S-mode — works only on QEMU) | `sstc` in `riscv,isa-extensions` → `stimecmp`; `riscv,clint0` at `0xe400_0000` is M-mode only |
| timebase | `TICKS_PER_US = 10` | 24 000 000 Hz |
| harts | `MAX_CPUS = 4`, ids 0..3 | 8, `cpu-map` two clusters (4 used until TASK-0330) |
| memory | fixed windows above `0x8000_0000` | banks at `0x0` (2 GiB) and `0x1_0000_0000` (2 GiB), reserved: OpenSBI `0–0x7ffff`, rcpu `0x100000–0x5fffff`, `dpu_reserved@2ff40000`, `framebuffer@7f000000` |
| link address | `0x8020_0000` (kernel), nxboot self-relocates to `0x9200_0000` | FIT load `0x0020_0000` for the payload; no fixed home |
| boot/display mode | fw_cfg (syscalls 45/50) | `/chosen/nexus,*` written by nxboot |
| device discovery (init) | probe the virtio window `0x1000_1000 × 8`, IRQ = slot index + 1/+3 | nodes by compatible with `reg` + `interrupts` |

## Goal

The kernel boots QEMU `virt` and the board from ONE binary with no cfg: `hal/platform.rs`
is filled at `kmain` from the FDT (`nexus-fdt`), every listed literal is deleted, the kernel
and nxboot are position-independent, syscalls 45/50 read `/chosen`, and init reads the tree
through a read-only FDT VMO to discover and grant devices. Proof: every QEMU profile prints
`KSELFTEST: platform from fdt ok (uart=… plic=… tb=…Hz harts=…)`; the board's serial shows the
kernel banner and the same marker (via TASK-0327B once B1.6 boots it).

## Non-Goals

SoC clocks/resets/pinmux/power (TASK-0245B); the page-frame allocator (TASK-0286 — this task
only stops the kernel from assuming where RAM is, by taking the banks from the tree into the
same structure 0286 replaces); 8-hart scheduling (TASK-0330); a userspace UART service; the
display-mode authority move (TASK-0251).

## End state (binding)

- `source/kernel/neuron/src/hal/platform.rs` (replaces `hal/virt.rs`): `Platform { uart:
  Uart{base, stride, clock_hz}, plic: Plic{base, ndev, s_context_of_hart[]}, timer:
  TimerSource::{Sstc, Sbi}, timebase_hz, harts: [HartId; MAX_CPUS] + count, memory: banks +
  reserved, chosen }` built once from the FDT; `hal/mod.rs` traits keep their shape, the
  impls take the struct. UART driver understands both register strides; the PLIC driver takes
  its S-mode context ids from `interrupts-extended`; the timer uses `stimecmp` when the ISA
  says so, else SBI `set_timer`; `TICKS_PER_US` becomes a runtime value (budgets in
  `core/trap/budgets.rs` are expressed in µs and converted once).
- **Position-independent kernel and nxboot**: static PIEs (link base 0, `-pie` link; codegen
  stays medany so code is PC-relative), an early fixup loop over `R_RISCV_RELATIVE`, the link
  address a symbol not a machine constant; nxboot chooses the kernel window from the tree
  (lowest free 2 MiB-aligned window of the first bank) and the measured record travels in
  `/chosen`; `core/boot_image.rs` knows the kernel's own physical range from the load address
  and M1 excludes it from the allocator's banks.
- `/chosen/nexus,boot-profile` and `nexus,display-mode` replace the fw_cfg reads behind
  syscalls 45/50 (50 itself dies in TASK-0251); the kernel has no fw_cfg code left.
- Init: the kernel maps the DTB read-only into init (a `device.fdt` capability, slot in
  `nexus-service-topology`, gate `check-slot-ssot.sh`); `helpers.rs` probes are replaced by
  an FDT walk that yields `(compatible, reg, irq)` per device class; IRQ numbers come from
  `interrupts`, the virtio `+1/+3` arithmetic is deleted; `route_provision.rs` finds the RTC by
  compatible (`google,goldfish-rtc` on QEMU; the board's RTC is TASK-0245B's / target picture N).
- Gate: `scripts/check-no-platform-literals.sh` fails on `0x1000_0000`, `0x0c00_0000`,
  `0x0200_0000`, `0x1000_1000`, `0x0010_1000`, `TICKS_PER_US = 10` outside the goldens.

## Packages

- **P0** — this recut; measured table above.
- **P1 Platform struct + UART/PLIC/timer from the FDT — built 2026-09-22.** `hal/platform.rs`
  (lock-free statics filled once by `init_from_fdt`, paging off, before the first log line:
  `early_boot_init(hartid, dtb)` zeroes BSS → records the tree → builds the platform → logs);
  `hal/virt.rs` deleted. Console: base/`reg-shift`/`reg-io-width` from the `stdout-path` node
  (byte-stride `ns16550a` and the board's 4-byte-stride part are one driver); bytes before init
  are dropped, never written to a guess; `diag/uart.rs` holds no address. PLIC: base and the
  S-mode context of every cpu READ from `interrupts-extended` (`2·hart+1` is gone), enable
  bitmaps cleared up to `riscv,ndev`. Timer: `stimecmp` (CSR 0x14d) when the ISA lists Sstc,
  SBI `set_timer` otherwise (`KINIT: timer sstc|sbi`); the CLINT MMIO writer is deleted;
  `DEFAULT_TICK_CYCLES` → `default_tick_cycles()` = 10 ms of the tree's timebase;
  `TICKS_PER_US` → `ticks_per_us()`; tick↔ns conversions exact and 64-bit via gcd-reduced
  factors (100/1 at 10 MHz, 125/3 at 24 MHz; the u128 form measured no different but was
  replaced on principle). Identity windows for UART/PLIC from the tree (`kernel_layout.rs`),
  the UART VA check in `address_space.rs` likewise; neuron-boot prints nothing itself.
  **Proof:** smp1 green — `KSELFTEST: platform from fdt ok (uart=0x10000000 plic=0xc000000
  ndev=95 tb=10000000Hz harts=1 … timer=sstc ticks_per_us=10 …)`, `KINIT: timer sstc`; kernel
  host tests (56) incl. the conversion tests; `just lint-kernel` clean. The MTTCG smp lane ran
  green on `per-hart ticks ok` / `runtime timer budget ok` / `bkl budget ok` under Sstc on two
  harts, but its `ipc call budget` read 243–344 µs — and **so did the unchanged HEAD in the same
  hour** (a runaway host process at 100 % CPU for six days; recorded in TASK-0054C): a host-load
  reading, not a P1 regression. **Chain 2026-09-22 (idle host):** `just check` 11/11, `just
  test-host` green, `just test-all` 27 PASS / 0 FAIL — every profile prints the FDT marker. One
  inner boot (`input-flood`'s visible boot) read `ipc call budget FAIL (rt=77us)` with the two-trap
  bench at 68 µs, the plain `visible` boot three minutes earlier at 42/42 on the same binary: the
  known 0054C wall-clock flake (second occurrence, first on 2026-09-21 before P1 existed).
- **P2 Position-independent kernel + nxboot — built 2026-09-22.** Both are static PIEs linked
  at 0 (`-pie --no-dynamic-linker -z notext -z norelro` from their `build.rs`; codegen stays
  medany/static, so code is PC-relative and the linker turns only the absolute words in data
  into `R_RISCV_RELATIVE` — the kernel's table holds 974 entries and no other type); `_start`
  of each applies its table with `lla`-based PC-relative addressing before touching a static.
  nxboot no longer copies itself to a home: it runs where the firmware loads it, reads the tree
  FIRST (`platform::init`: console from `stdout-path` with `reg-shift`/`reg-io-width`; the
  loader's UART, virtio window and fw_cfg literals are deleted — transports by `virtio,mmio`,
  fw_cfg by `qemu,fw-cfg-mmio`), and chooses the kernel window: the lowest 2 MiB-aligned
  `LOAD_MAX` window of the lowest bank clear of `/reserved-memory`, the memreserve block, the
  tree and its own image (`0x8040_0000` on virt, above the loader; `0x0060_0000` on the board
  above OpenSBI + rcpu). `nxboot: jump slot=a base=0x…`. NXBD `load_addr` becomes
  `LOAD_ADDR_RELOCATABLE` (`u64::MAX`), the only value the loader boots — the fixtures' fixed
  address stays a reject. The measured handoff record travels as `/chosen/nexus,boot-record`
  (60 bytes, ADR-0059 v1 layout); the fixed page, `bootfmt::handoff::{ADDR, PAGE}` and the
  kernel's `HANDOFF_ADDR` asserts are deleted. Kernel `core/boot_image.rs` prints
  `KSELFTEST: kernel image ok (base=0x… len=0x… relocs=N)` (FAIL names an unsupported type),
  REQUIREd in every profile. **Proof (2026-09-22):** the same kernel booted at two addresses —
  smp1 through nxboot: `nxboot: jump slot=a base=0x80400000`,
  `KSELFTEST: kernel image ok (base=0x80400000 len=0x11e5b80 relocs=982)`, `boot handoff ok
  (measured)` read from `/chosen`; `NEXUS_DIRECT_KERNEL=1` headless: `kernel image ok
  (base=0x80200000 … relocs=982)`, `boot handoff absent (direct kernel)`, `init: ready`. The
  direct lane then stalls in userspace (no `/chosen` profile: nxboot is the only fw_cfg reader
  since TASK-0244) and its ladder requires the nxboot rungs — the dev path is not a proof lane;
  the marker line is the second-address evidence. Measured for M1 (TASK-0286): 106 `VA == PA` pointer casts in 32 files;
  RAM at physical 0 on the board puts an identity-mapped kernel inside the user VA range, so
  M1's P0 decides the kernel direct map at a VA offset (not P2's: it is one change with the
  allocator). ADR-0059 amended, RFC-0089 §5/§7 and RFC-0098 C2 rows updated.
- **P3 `/chosen` syscalls + fw_cfg deletion from the kernel — built 2026-09-22.** nxboot
  re-expresses `selftest-mode` as `nexus,boot-mode` (beside `boot-profile`/`display-mode`);
  `diag/boot_mode.rs` resolves marker folding (syscall 45) and the display request (syscall 50)
  from `/chosen` via `boot_fdt::bytes()`; the kernel's fw_cfg MMIO reader and the fw_cfg identity
  window in `kernel_layout.rs` are deleted — `grep fw_cfg source/kernel` is empty. Host tests for
  the mode words and `WxH` parsing. Not P3's: init still grants selftest-client the fw_cfg window
  by a literal (`orchestrator.rs`) and selftest-client reads its profile there — P4 replaces both
  with the `device.fdt` VMO.
- **P4 Init discovery from the FDT VMO — built 2026-09-22.** The kernel injects a `VmoRo`
  alias of the (page-aligned, nxboot-allocated) tree into init's slot 2
  (`nexus_abi::INIT_DEVICE_TREE_SLOT`); `bootstrap/device_tree.rs` maps it once and yields the
  `virtio,mmio` transports (classified by device id through a short-lived window each, lowest
  address first) and the RTC by compatible; `helpers.rs`'s window scan and `route_provision.rs`'s
  RTC literal are deleted. **The PLIC line travels inside the device capability:**
  `DeviceMmio { base, len, irq }`, `device_mmio_cap_create(base, len, irq, slot)`, `cap_query`
  reports `irq` (and read-only aliases as kind 3) — virtio-blk, gpud and hidrawd take their line
  from the capability; the `index + 1` / `3 + idx` / `GPU_IRQ_SOURCE = 8` arithmetic is gone.
  The harness reads `/chosen/nexus,boot-mode|boot-profile` through the same alias
  (`NamedSlot::DeviceTree` replaces `FwCfg`; init's fault fixture reads the profile from its own
  view); `nexus_abi::fwcfg`, the fw_cfg grant, the dead `sink-kernel` arm of nexus-log and the
  last UART constants (kernel fault dumper, init's early writer via `debug_putc`) are deleted.
  Gate: `scripts/check-no-platform-literals.sh` (self-tested scanner over `source/`,
  `userspace/`, `tools/nx/src`; comments, tests and goldens excluded) in `just check`. Marker
  `init: devices from fdt ok (virtio=N blk_irq=… gpu_irq=… rtc=…)` required in every profile.
  Not done here (follow-up in this ledger's DoD sweep): `sys_irq_bind` still accepts any line a
  task names — binding should require a device capability carrying that line. Measured while
  writing the `cap_query` test (`syscall/api/tests_devcap.rs`): the whole `syscall` module is
  `cfg(target_os = "none")`, so every kernel API test file is host-INERT (58 host tests run, none
  of them API tests) — the query is proven by the boot instead (`gpud: gpu irq bound`,
  `hidrawd: irq endpoint bound`, the disk's IRQ in `init: devices from fdt ok`); un-gating the
  API tests is the orchestrator track's known host-testability item. **Proof (2026-09-22):**
  smp1 + visible green with `init: devices from fdt ok (virtio=8 blk_irq=5 gpu_irq=8
  rtc=0x101000)`, `virtioblkd: irq endpoint bound`, `gpud: gpu irq bound` / `gpu irq wake`,
  `hidrawd: irq endpoint bound (reactive input)`, `timed: walltime anchored`,
  `init: device tree grant ok svc=selftest-client`; `just check` 12/12 with the literal gate.
  **Measured in the `input-flood` lane (2 of 2 red under P4, 1 of 3 before):** the marker's
  `input_irqs=3,4,0` equal the lines hidrawd's old `3 + idx` arithmetic bound for the two live
  input devices, and `blk_irq=5` / `gpu_irq=8` equal the old `index + 1` / constant — P4 changed
  NO interrupt binding on QEMU virt; a first hypothesis (the tablet newly reactive) is refuted by
  this line. The two reds are the TASK-0054C class (the inner boot 1.6× slower, bench 68/82 µs,
  budget 65–69 µs) with a new consequence: the ladder's early stop ended the VM under the 45 s
  flood and reset the QMP socket. Fix in `scripts/input-flood-lane.sh` (0054C's own
  prescription): no early stop for the inner boot, the flood starts once THIS run's
  `SELFTEST: ipc bench (` has printed, `RUN_TIMEOUT` 320 s — the lane alone: bench 42 µs,
  `[PASS] input-flood`.

## Constraints / invariants

- The kernel keeps `MAX_CPUS` as a compile-time ceiling and parks harts beyond it through SBI
  HSM (the board's 8 harts are TASK-0330's).
- No CLINT MMIO from S-mode, ever (PMP-fenced on silicon).
- Every deleted literal has a test that would fail if it came back (the grep gate).
- Warnings gate, `forbid(unsafe_code)` outside the documented kernel modules.

## Red flags / decision points

- **RED (decides the timer):** Sstc must be verified on QEMU (`-cpu max` lists it) and on the
  board (`riscv,isa-extensions` has `sstc`) before the SBI fallback is the only path; both are
  kept, the FDT chooses.
- **YELLOW:** a PIC kernel changes the linker script and the early-boot assembly — the smp1
  and visible lanes are the regression net; measure boot time before/after (0269B's number).
- **GREEN:** the board's UART is 16550-class at a 4-byte stride (mainline binds it as
  `intel,xscale-uart`); one driver, two strides.

## Definition of Done

`just test-all` green with every QEMU profile printing the FDT marker; the literal gate in
`just check`; two-load-address boot proven; the kernel has no fw_cfg code; init discovers
virtio devices from the tree with IRQs from `interrupts`; docs (`docs/architecture/01-neuron-kernel.md`
platform section, RFC-0098 Phase 1 ✅, CHANGELOG).

**Sweep 2026-09-22:** all of the above ✅ on QEMU (P1–P4). Added in the sweep: `irq_bind` and
`irq_complete` accept only a line the caller's device capability carries
(`CapTable::holds_device_irq`; negative proof `SELFTEST: irq bind deny ok` in the ipc_kernel
phase, required in every profile; the positive half is every driver's own bind marker);
`docs/architecture/01-neuron-kernel.md` boot flow + HAL rewritten around the tree with a
"Platform from the device tree" section. RFC-0098 Phase 1 is 🟨: this task ✅, 0245B open.
The board's serial shows these markers only after B1.6 (nxboot as FIT payload) — TASK-0327B.
