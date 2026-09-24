---
title: TASK-0286 Kernel memory v1a (M1): the physical map comes from the FDT and a page-frame allocator replaces the fixed windows — page-backed VMOs, a `contiguous-DMA` kind, and the accounting counters this ledger always promised
status: In Progress (P3b done 2026-09-23 — no fixed physical window left, `-m` a lane knob, gate in `just check`; P3a done 2026-09-23 — the VMO is a page-backed object, `VmoPool` + the arena deleted; P2b done 2026-09-23 — frames live at boot, page tables are frames, the kernel half is shared; P2 done 2026-09-23 — the kernel runs in the high half, smp1 + visible green; P1 done 2026-09-22 — `frames` host-proven over both golden trees; P0 done 2026-09-22 — measured, the kernel direct map decided; recut 2026-09-22 to the end state — Block 1 B1.4 of the hardware fast track and M1 of target picture M; was "per-task RSS counters + pressure snapshots + trusted query ABI", Draft since 2026-04-13)
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
- **P2 Direct map — done 2026-09-23.** `mm/phys.rs` (top-level, host-tested: `PHYS_OFFSET`,
  `KERNEL_VA_BASE`, `PAGING_ON`, `phys_to_virt`/`virt_to_phys`/`is_kernel_va`, the boot table
  builder `build_boot_table` — 1 GiB leaves for every bank, both device windows, the tree and
  the identity gigabyte — proven for both goldens' shapes). Two-phase entry in `neuron-boot`:
  fixups at the load PA → `early_boot_init` at PA (BSS, tree, platform incl. the `/memory`
  banks now recorded in `hal/platform`, the boot table) returns `satp` → `_start` writes it,
  jumps to `PHYS_OFFSET + PC`, re-applies the fixups with the high base → `high_boot_init`
  (seam on, traps, timer, heap; `KINIT: kernel high half (base=0xffffffc080400000
  load=0x80400000 …)`). Secondary stub: `lla` only, switches to the published boot `satp`,
  jumps high, then `sp`/`gp`/Rust; `hart_start` gets the physical entry. `kernel_layout.rs`
  rewritten (130 lines, was 301): the image at its alias with segment permissions and the
  stack guard, every bank through the direct map minus the image, the two windows, the tree
  when outside a bank — the identity map, the pool/arena/stack-pool identity windows and
  `AddressWindow` are gone. Every physical dereference goes through the seam (page-table walk
  + `root_ppn` + child PPNs, `vmo`, `vmo_pool`, `exec`, `exec_copy`, `stack_pool`, `phased`,
  `fault`, the trap-time page walk, the selftests, `boot_fdt::bytes`, console + PLIC MMIO);
  "user address" = `!is_kernel_va(sepc)` in `cpu_main` and the handler; the `0x8000_0000..`
  GLOBAL hack in `map_page` deleted; the selftest's data-page cap carries `virt_to_phys` of
  the static (a kernel pointer is not a frame). Found by the boot, not by reading: the "vmo
  zero" selftest probed the VMO at its PA. The YELLOW risk held: no Rust runs between the
  `satp` write and the second fixup pass. The proof profiles run ONE hart; the secondary
  switch is proven by `just ci-os-smp` (SMP=2, MTTCG): `KGATE: smp hart1 online=1 asm=1
  stage=4`, `smp bringup ok mask=0x3`, `KSELFTEST: smp online ok`. That lane's verdict stays
  red on `ipc call budget FAIL (rt=249–326us budget=64)` — measured as PRE-EXISTING: the same
  lane on 2026-09-22 before P1/P2 read 195–344 µs (eleven runs in `build/logs/smp--2026-09-22*`);
  the budget is wall-clock class (TASK-0054C follow-up "budget as ratio"), not this package.
- **P2b Frames at boot — done 2026-09-23.** `mm/frame_pool.rs`: the ONE `FrameAllocator`
  behind a leaf lock, built in `high_boot_init` after the heap from the tree — carved out: the
  tree's reserved ranges, the image, the tree, and the windows the old owners still hold until
  P3 (`KERNEL_PAGE_POOL`, `USER_VMO_ARENA`, the user stack pool, the bootstrap identity
  window, now `mm::BOOTSTRAP_IDENTITY_WINDOW`); `alloc` logs `MM: frames exhausted
  (want=… free=…)`, `free` logs a refused free; `KINIT: mm frames (banks=1 total=13378
  free=13378 reserved=96 excluded=68542)` on virt (52 MiB free while the windows still exist).
  Page tables are frames: `PageTable::alloc_page` takes an order-0 block through the direct
  map and zeroes it, `Drop` returns it; the `bringup_identity` static pool (64 image pages)
  and `pt_static_root` are DELETED (features gone from both Cargo.tomls; the boot crate had
  it on, which is why the first measurement showed `pt_live=0` — the pool served the first 64
  tables silently). The kernel half is built once (`kernel_layout::KERNEL_ROOT`) and every
  later root adopts entries 256..512 (`adopt_kernel_half`): the kernel address space costs 6
  page-table frames on virt (`KINIT: mm frames in use (free=13372 allocs=6 frees=0
  pt_live=6)`), a user address space its root plus its own user-level tables — the satp of
  every activation names a frame outside the image now. Stats renamed
  `PageTableAllocationStats { live, total, peak }` (they were `heap_*`). Allocation policy
  pinned by a host test with the boot's exact exclusions: classic buddy — the smallest
  sufficient order first, the lowest block within it (an exact-size free block anywhere beats
  splitting a bigger one lower down), deterministic across boots.
- **P3a Page-backed VMO — done 2026-09-23.** `mm/vmo.rs`: `VmoObject { kind, len, blocks }`
  in a table of ≤ 4096 objects behind a leaf lock; kinds `Anon` (pool blocks, largest first:
  `frame_pool::alloc_bytes`), `Contiguous` (ONE block — the DMA masters' kind; `vmo_create`
  arg 2 bit 0, `nexus_abi::vmo_create_contiguous`; `cap_query` reports its physical base, an
  `Anon` object reports 0), `Fixed` (frames outside the pool: the tree alias, the selftest data
  page, the bootstrap identity window — never freed). Capabilities are `Vmo { id, len }` /
  `VmoRo { id, len }`; `vmo_ref_count(id)` replaces the range-overlap count; `VaRegion` carries
  the id and `any_backed_by_vmo` is the destroy guard; `vm_map` maps the object's runs back to
  back (`vm_ops::map_runs`, one region, promotion per run — largest-first keeps every 2 MiB run
  superpage-aligned); `vmo_read/write` copy run by run; the legacy `as_map` translates per page.
  Create is phased as before: A takes the frames, B zeroes them with the BKL dropped, C installs
  the cap. `exec` images are pool blocks: `exec_image.rs` (`alloc_image`, `plan_payload` split
  across blocks, `map_blocks`, `alloc_zeroed_page`), `ImageAllocs` records `Block`s (48),
  teardown frees them to the pool; the copy plan holds 64 ops. DELETED: `vmo_pool.rs`,
  `USER_VMO_ARENA_*`, the idle zero-frontier (`vmo_idle_zero_step` + the cpu_main hook: every
  object is zeroed at create, off the BKL), `log_vmo_preview`, the arena layout assert.
  Frame ceiling raised to `MAX_ORDER = 15` (128 MiB) with `SUPERPAGE_ORDER = 9` kept for the
  alignment guarantee — measured: windowd's scanout+atlas resource is 1280 × 9600 × 4 = 49 MiB
  and rounds to a 64 MiB block (the virtio-gpu backing is scatter-gather capable; P4 attaches
  the runs and the resource becomes `Anon`). DMA users switched to contiguous: virtio-blk,
  virtio-rng, virtio-input, virtio-net (`nexus-net-os`), gpud (queues, resource backings,
  scratch), windowd's framebuffer, app-host's surfaces. On virt the pool is now 70 721 frames
  (276 MiB) free at boot, 11 199 excluded (image, tree, page pool, stack pool, identity
  window). Proof: `KSELFTEST: vmo zero ok`, `vm map ok` (a contiguous object, promotion
  proven), `vm unmap ok`, `vm map reject ok`; smp1 9/9 chain markers, `desktop revealed`.
  Exhaustion still returns the arena's errno (EPERM-class) — the ENOMEM class is M4's.
- **P3b Windows gone — done 2026-09-23.** The init loader's pages (`alloc_init_page`) are
  frames; the non-exec spawn stack is ONE order-2 block recorded on the task's `ImageAllocs`
  (returned with its image); the bootstrap identity VMO (PID 0's cap slot 1) is deleted —
  found by the boot: its one user was the kernel selftest, which mapped the child address
  space's STACK from it, i.e. writable pages over the firmware's first megabyte (the child
  stack is a VMO of its own now, slot 6). `KERNEL_PAGE_POOL_*`, `STACK_POOL_*`, `BOOTSTRAP_IDENTITY_WINDOW`
  and the `StackPool` cursor are gone — `mm/mod.rs` holds no physical address constant; the
  pool excludes only the image and the tree. `kmain::assert_memory_layout` (the P0.1 layout
  tripwire: image vs pool vs arena, `KERNEL: layout ok`, `NEURON_LAYOUT_PAD`) and its lane
  `scripts/contract-image-layout.sh` + `just contract-image-layout` are deleted: there is no
  window an image could grow into; the pool report is the truth. `-m` is
  `QEMU_MEM` in the launcher (default 320M); `QEMU_MEM=256M` and `QEMU_MEM=1G` smp1 boots
  are the proof that nothing depends on the size. Gate `scripts/check-no-fixed-windows.sh`
  in `just check` (`fixed-windows`): the retired names and any `0x8xxx_xxxx` RAM literal in
  the kernel outside comments, test modules and the RFC-0085 VA limit; self-tested on
  fixtures. Left for P4: the physical-cast rule of the gate ("no `as *mut` of a physical
  address outside `phys`") is a review rule, not a regex — every dereference goes through
  `phys_to_virt` today, measured by reading, and P4's `DmaBuffer` is the next writer.
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
