---
title: TASK-0286 Kernel memory v1a (M1): the physical map comes from the FDT and a page-frame allocator replaces the fixed windows — page-backed VMOs, a `contiguous-DMA` kind, and the accounting counters this ledger always promised
status: In Progress (P1 done 2026-09-22 — `frames` host-proven over both golden trees; P0 done 2026-09-22 — measured, the kernel direct map decided; recut 2026-09-22 to the end state — Block 1 B1.4 of the hardware fast track and M1 of target picture M; was "per-task RSS counters + pressure snapshots + trusted query ABI", Draft since 2026-04-13)
owner: @kernel-team @runtime
created: 2026-04-13
updated: 2026-09-22
depends-on:
  - tasks/TASK-0244-bringup-rv-virt-v1_0a-host-dtb-sbi-shim-deterministic.md
  - tasks/TASK-0245-bringup-rv-virt-v1_0b-os-kernel-uart-plic-timer-uartd-selftests.md
follow-up-tasks:
  - tasks/TASK-0286B (M2, seeded at its P0): demand paging + CoW fault path
  - tasks/TASK-0286C (M3, seeded at its P0): page cache + shared read-only code
  - tasks/TASK-0287-kernel-memory-pressure-v1-hard-limits-oom-handoff.md (M4)
links:
  - Contract: docs/rfcs/RFC-0098-board-support-contract-fdt-truth-boot-chain.md (C4, Phase 2); target picture M in tasks/IMPLEMENTATION-ORDER.md; RFC-0100 (memory object model v2, seeded at M2's P0)
  - What this deletes: source/kernel/neuron/src/mm/mod.rs (`USER_VMO_ARENA_*`, `KERNEL_PAGE_POOL_*`), syscall/api/vmo_pool.rs (`VmoPool`)
  - What stays: docs/rfcs/RFC-0085-kernel-owned-va-allocation.md (VA side), docs/rfcs/RFC-0080-* (`vmo_share_ro`), docs/adr/0054-*
  - Absorbs: tasks/TASK-0284-userspace-dmabuffer-ownership-v1-prototype.md (the `contiguous-DMA` kind + `DmaBuffer` hooks)
  - Measurement: docs/board/measurements/2026-09-22-stock-system/README.md ("Memory", "DMA coherence")
  - Playbook: CLAUDE.md
---

## History

Seeded 2026-04-13 as accounting counters + pressure snapshots (a release blocker on the board).
Recut 2026-09-22: counters need something to count. The physical allocator is the first
memory package of the hardware track and the foundation of every later M step; the counters
this ledger promised ride along.

## Context (measured 2026-09-22)

Physical memory in the kernel is three compile-time windows on QEMU's `0x8000_0000`-based map
(`USER_VMO_ARENA` 224 MiB at `0x8380_0000`, `KERNEL_PAGE_POOL` 24 MiB, `-m 320M`), carved by a
bump + 16-slot free list (`VmoPool`); a VMO is a physical range + a capability, committed on
create. The board has **4 GiB in two banks at physical 0 and 4 GiB** (`memory@0` 2 GiB,
`memory@100000000` 2 GiB), reserved ranges below 6 MiB (OpenSBI, rcpu), and DMA masters that
are not cache-coherent (Zicbom/Svpbmt in the ISA, `swiotlb` in the stock kernel). None of the
windows can survive; relocating them by the FDT would be an interim.

**Measured 2026-09-22 (TASK-0245 P2, for this ledger's P0):** the kernel identity-maps every
physical range it owns (its image, the windows, MMIO) as GLOBAL pages — 106 raw-pointer casts
of physical addresses in 32 kernel files rely on `VA == PA`. User VAs live below
`USER_VADDR_LIMIT` (2 GiB). On the board, RAM starts at physical 0, so an identity-mapped kernel
and its windows would sit INSIDE the user VA range: the identity map cannot survive either. M1's
P0 decides the kernel direct map at a VA offset (a `phys_to_virt` seam replacing the 106 casts;
RFC-0098 C4 amendment) together with the allocator — the two are one change, because every
consumer of a physical address is touched once. P2 left the kernel position-independent
(load address any; VA still == PA) so the seam is the only thing left to move.

**Measured 2026-09-22 (M1 P0).** `VmoPool` has 22 use sites in 7 files (`vmo.rs`, `exec.rs`,
`task_image.rs`, `kernel_layout.rs`, `lib.rs`, `api/mod.rs`, tests); the fixed windows have 35
sites in 10 files (incl. `stack_pool.rs`, `selftest/{mod,vm_alloc}.rs`, `va_space.rs`, `kmain.rs`).
Of the 106 raw-pointer casts, the physical ones are: the page-table walk (`page_table.rs`, 11:
`(entry >> 10) << 12` as a pointer, and `root_ppn` = pointer / 4096), `kernel_layout.rs` (9),
`exec.rs` + `exec_copy.rs` (11: arena destinations), `vmo.rs` + `vmo_pool.rs` (10), the
selftests (13), the trap handler's page-walk dump (3), `hal/platform.rs` + `plic.rs` (6: MMIO
windows), `kmain.rs`/`boot_image.rs`/`boot_fdt.rs` (the image and the tree). The rest are user
VAs (IPC copies into the caller's space, `ensure_user_slice`-checked), function pointers, or
host-test layout asserts — untouched by a direct map. Page-table pages come from a 64-page
static pool and then the kernel heap (8 MiB `.bss.heap`), so they live inside the image; the
kernel heap stays where it is. Two more UART literals surfaced in `trap/handler.rs` (written
`0x10000000`, a spelling the literal gate did not match — fixed with this P0).

## Goal

Physical memory comes from the tree and the kernel lives in the high half. A page-frame
allocator owns every FDT memory bank minus reserved ranges, the kernel's own image and the
tree (`mm/frames.rs`: buddy over 4 KiB frames with 2 MiB order support, per bank, host-tested;
`init_from_fdt(banks, reserved, excluded)`). The kernel is mapped at a VA offset — a direct map
`KVA = PHYS_OFFSET + PA` of every bank and every device window in the Sv39 high half — so the
user half is the user's on every machine, RAM at physical 0 included; the identity map is
deleted and `phys_to_virt`/`virt_to_phys` are the one seam the physical casts become. The VMO
becomes a page-backed object (`mm/vmo.rs`: `VmoObject { frames, kind: Anon | ContiguousDma,
len, owner }`) whose `vm_map` (RFC-0085) maps its frames; `VmoPool` and the fixed windows are
deleted. `kind = ContiguousDma` is the only kind that allocates physically contiguous frames —
for framebuffers, ADMA rings, USB rings, GPU buffers — and carries the coherence attribute
(RFC-0098 C4: no `dma-coherent` on the node ⇒ `DmaBuffer::for_device/for_cpu` do Zicbom
maintenance, or the mapping is Svpbmt non-cacheable). Accounting: per-address-space
`frames_mapped`, `vmos_committed`, `dma_frames`, global `free`/`total` per bank;
`KSELFTEST: mm frames (banks=… total=… free=…)` printed after bring-up; a trusted query (the
existing sched/mem telemetry syscall shape) exposes the counters read-only to metricsd.

## Non-Goals

Demand paging, CoW, page cache, purgeable, pressure levels, compression, swap (M2–M7);
NUMA; huge pages beyond the 2 MiB superpage promotion that already exists; changing the VA
side (RFC-0085).

## End state (binding)

- **Direct map (P2).** `PHYS_OFFSET = 0xffff_ffc0_0000_0000` (the Sv39 kernel half; covers every
  PA below 256 GiB on both machines). `_start` applies the PIE fixups at the load PA, early Rust
  runs at PA (BSS, the tree, the platform, the memory banks), builds a boot table of 1 GiB
  pages — RAM banks cacheable, everything else with the device attribute where the ISA lists
  Svpbmt — plus an identity window for the switch, writes SATP, jumps high and applies the
  fixups again with the high base (`R_RISCV_RELATIVE` is idempotent: addend + base). From there
  every kernel VA is `PHYS_OFFSET + PA`: the image, the frames, the page tables (allocated from
  frames; `root_ppn` via `virt_to_phys`), the windows (`console_write_byte` through the direct
  map once a `PAGING_ON` static is set), the tree. `map_kernel_segments` maps the image (text RX,
  data RW), the banks and the device windows into every address space as GLOBAL high-half
  entries; nothing kernel-owned lies below `KERNEL_VA_BASE`, and "user address" =
  `va < KERNEL_VA_BASE` everywhere `0x8000_0000` used to decide it. Secondary harts run the same
  switch from `__secondary_hart_start`. RFC-0085's user window stays (2 GiB, unchanged).
- `mm/mod.rs` has no address constant; `mm/frames.rs` + `mm/vmo.rs` replace `vmo_pool.rs`;
  `sys_vmo_create` takes a kind; `vmo_share_ro`, `vm_map/unmap`, `vmo_read/write`, `exec`'s
  image allocation (still a copy until M3) run over frame lists; `stack_pool.rs` allocates from
  frames.
- Idle zeroing (`idle_zero_step`) becomes zeroing of freed frames into a zeroed pool;
  exhaustion is an event (`MM: frames exhausted (want=… free=…)`, RFC-0087) — never silent.
- `-m` is no longer pinned to 320M in the launcher; a `-m 1G` and a `-m 256M` QEMU boot both
  pass the smp1 ladder (the allocator sizes itself from the tree).
- Gate `scripts/check-no-fixed-windows.sh` (no `USER_VMO_ARENA`, `KERNEL_PAGE_POOL`, `VmoPool`,
  no `as *mut`/`as *const` of a physical address outside `mm::phys`) in `just check`.

## Packages

- **P0 — done 2026-09-22.** Measured map above; the direct-map decision (RFC-0098 C4 amended);
  the literal gate's spelling gap closed with the two handler UART constants.
- **P1 Frame allocator (host) — done 2026-09-22.** `mm/frames/{mod,bank}.rs` (top-level
  `pub mod frames` with a `#[path]`, host-compiled like `image_allocs`; `mod mm` is OS-only):
  a buddy per bank over a bitmap per order plus a one-bit-per-word summary, indices from the
  bank base rounded down to 2 MiB so every order-9 block is superpage-aligned whatever the
  bank base; first-fit in bank order (deterministic: the same tree and calls yield the same
  frames — tested); `init(banks, reserved, excluded)` / `init_from_fdt(&Fdt, excluded)`
  (≤ 4 banks, ≤ 32 holes; a hole poisons whole frames); `alloc(order)`, `alloc_below(order,
  limit)` (a 32-bit DMA master: first-fit makes "the lowest block is above the limit" exact),
  `free(block)` refusing foreign, misaligned, hole-touching and double frees (`NotOwned` /
  `NotAllocated`; a partially free parent is a debug assertion); `Stats { banks, total, free,
  reserved, excluded, allocs, frees, exhausted }` — exhaustion is `Err(Exhausted { order,
  free })` AND a counter, the log line lands with the kernel wiring in P2. Metadata is a boxed
  slice (128 KiB per 2 GiB bank), never the frames themselves, so the board tree runs on the
  host with no memory behind it. 12 host tests: both goldens (virt: one bank, 81 920 frames;
  board: two banks around the 2 GiB hole, OpenSBI's 128 frames reserved, first superpage at
  2 MiB, bank 1 only after bank 0), exclusions never handed out, determinism, merge round trip
  (14 allocs/frees → the same first superpage), the reject matrix, exhaustion counting, hole
  frame-poisoning, an unaligned bank base, table overflow, and a proptest over random
  alloc/free scripts (no overlap, `free + live == total`, everything comes back).
- **P2 Direct map.** The boot switch, `mm/phys.rs` (`phys_to_virt`/`virt_to_phys`, `PAGING_ON`),
  page tables from frames, `kernel_layout.rs` over banks + windows, every physical cast moved,
  identity map deleted; smp1/visible green; the kernel prints `KINIT: kernel high half
  (base=0xffff…)`.
- **P3 Page-backed VMO.** `VmoObject` + frame lists; every syscall path moved; `VmoPool`, the
  windows and `stack_pool`'s window deleted; `-m` unpinned; 256M and 1G proven.
- **P4 `contiguous-DMA` + coherence hooks.** The kind, `DmaBuffer::for_device/for_cpu` (no-op
  on QEMU), gpud's framebuffer and virtio rings moved onto it (absorbs TASK-0284).
- **P5 Telemetry + gate + docs.** `KSELFTEST: mm frames (…)` registered; read-only query to
  metricsd; `check-no-fixed-windows.sh`; `docs/architecture/01-neuron-kernel.md` memory section.

## Constraints / invariants

- RFC-0085's VA rules unchanged (RECORD-THEN-MAP, CLEAR-SHOOTDOWN-FORGET).
- Every frame has one owner (a VMO or the kernel); double free is a kernel assertion in debug
  and an event in release.
- Determinism: the allocator's order of frames for a given tree is stable (the smp1 lane
  depends on it).
- BKL budgets hold (`KSELFTEST: bkl budget ok`); an allocation costs at most orders × summary
  words (10 × 128 word reads for a 2 GiB bank, one word + `ctz` after that), a free costs at
  most `MAX_ORDER` merge steps — no list walk, no per-frame scan.

## Red flags / decision points

- **RED:** `exec` still copies code per process until M3; with 4 GiB this is not a blocker for
  the first picture but the RSS number (R10) is measured here for M3's gate.
- **YELLOW:** superpage promotion (`vm_ops.rs`) assumes physically contiguous ranges — page
  lists must expose runs so promotion survives; a test pins it.
- **YELLOW (P2):** the boot switch runs Rust before the second fixup pass — only PC-relative code
  and no absolute pointer from data may run there (the early platform code already obeys this,
  it was written for paging-off); the switch is proven by the boot, on both `-m` sizes and with
  SMP=2 (secondary harts take the same path).
- **GREEN (measured):** two banks with a 2 GiB hole — the per-bank design is forced, not
  chosen.

## Definition of Done

Host tests; `just test-all` green with no fixed window; two `-m` sizes boot; the marker on
QEMU and (via B1.6) on the board's serial; the gate in `just check`; docs + CHANGELOG;
TASK-0284 closed as absorbed.
