---
title: TASK-0245 Board support v1b (OS): the kernel's platform comes from the FDT — UART, PLIC, timer, memory ranges, hart list; kernel + nxboot position-independent; init discovers devices from the FDT
status: In Progress (P1 Done 2026-09-22, chain 27/27 — `hal/platform.rs` replaces `hal/virt.rs`: console/PLIC/timer/timebase from the tree, Sstc chosen by the ISA list, no CLINT; smp1 green; recut 2026-09-22 to the end state — Block 1 B1.2 of the hardware fast track; was "Hardware Bring-up (RISC-V virt) v1.0b: kernel UART/PLIC/timer + userspace uartd + selftests", Draft since 2025-12-29)
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
- **Position-independent kernel and nxboot**: `-C relocation-model=pic`, an early relocation
  loop over `R_RISCV_RELATIVE`, the link address a symbol not a machine constant; `mm/
  kernel_layout.rs` computes the kernel's own physical range from the load address and excludes
  it from the allocator's banks.
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
- **P2 Position-independent kernel + nxboot** — boots at two different load addresses on QEMU
  (`-kernel` placement vs a moved FIT-style load) — the proof that no address is baked.
- **P3 `/chosen` syscalls + fw_cfg deletion from the kernel** (nxboot writes `/chosen` on
  QEMU from fw_cfg — TASK-0244 P3).
- **P4 Init discovery from the FDT VMO** — `helpers.rs`/`route_provision.rs` rewritten, slot
  + policy for `device.fdt`, IRQs from the tree; the literal gate lands and `just check`
  carries it.

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
