---
title: TASK-0286 Kernel memory v1a (M1): the physical map comes from the FDT and a page-frame allocator replaces the fixed windows — page-backed VMOs, a `contiguous-DMA` kind, and the accounting counters this ledger always promised
status: Draft (recut 2026-09-22 to the end state — Block 1 B1.4 of the hardware fast track and M1 of target picture M; was "per-task RSS counters + pressure snapshots + trusted query ABI", Draft since 2026-04-13)
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

## Goal

A page-frame allocator owns every FDT memory bank minus reserved ranges and the kernel's own
image (`mm/frames.rs`: buddy over 4 KiB frames with 2 MiB order support, per-bank, host-tested;
`init_from_fdt(banks, reserved, kernel_range)`), and the VMO becomes a page-backed object
(`mm/vmo.rs`: `VmoObject { pages: PageList, kind: Anon | ContiguousDma, len, owner }`) whose
`vm_map` (RFC-0085) maps its pages; `VmoPool` and the fixed windows are deleted. `kind =
ContiguousDma` is the only kind that allocates physically contiguous frames — for framebuffers,
ADMA rings, USB rings, GPU buffers — and carries the coherence attribute (RFC-0098 C4: no
`dma-coherent` on the node ⇒ `DmaBuffer::for_device/for_cpu` do Zicbom maintenance, or the
mapping is Svpbmt non-cacheable). Accounting: per-address-space `frames_mapped`,
`vmos_committed`, `dma_frames`, global `free`/`total` per bank; `KSELFTEST: mm frames
(banks=… total=… free=…)` printed after bring-up; a trusted query (the existing sched/mem
telemetry syscall shape) exposes the counters read-only to metricsd.

## Non-Goals

Demand paging, CoW, page cache, purgeable, pressure levels, compression, swap (M2–M7);
NUMA; huge pages beyond the 2 MiB superpage promotion that already exists; changing the VA
side (RFC-0085).

## End state (binding)

- `mm/mod.rs` has no address constant; `mm/frames.rs` + `mm/vmo.rs` replace `vmo_pool.rs`;
  `sys_vmo_create` takes a kind; `vmo_share_ro`, `vm_map/unmap`, `vmo_read/write`, `exec`'s
  image allocation (still a copy until M3) run over page lists.
- Idle zeroing (`idle_zero_step`) becomes zeroing of freed frames into a zeroed pool;
  exhaustion is an event (`MM: frames exhausted (want=… free=…)`, RFC-0087) — never silent.
- `-m` is no longer pinned to 320M in the launcher; a `-m 1G` and a `-m 256M` QEMU boot both
  pass the smp1 ladder (the allocator sizes itself from the tree).
- Gate `scripts/check-no-fixed-windows.sh` (no `USER_VMO_ARENA`, `KERNEL_PAGE_POOL`, `VmoPool`
  identifiers) in `just check`.

## Packages

- **P0** — this recut; measured map above; RFC-0100 seed deferred to M2 (this package changes
  no object semantics visible to userspace).
- **P1 Frame allocator** — host-tested buddy per bank; `init_from_fdt`; counters.
- **P2 Page-backed VMO** — `VmoObject` + page lists; every syscall path moved; `VmoPool`
  deleted; QEMU smp1/visible green; two `-m` sizes proven.
- **P3 `contiguous-DMA` + coherence hooks** — the kind, `DmaBuffer::for_device/for_cpu`
  (no-op on QEMU), gpud's framebuffer and virtio rings moved onto it (absorbs TASK-0284).
- **P4 Telemetry** — `KSELFTEST: mm frames (…)` marker registered; read-only query to
  metricsd; `docs/architecture/kernel-memory.md` rewritten.

## Constraints / invariants

- RFC-0085's VA rules unchanged (RECORD-THEN-MAP, CLEAR-SHOOTDOWN-FORGET).
- Every frame has one owner (a VMO or the kernel); double free is a kernel assertion in debug
  and an event in release.
- Determinism: the allocator's order of frames for a given tree is stable (the smp1 lane
  depends on it).
- BKL budgets hold (`KSELFTEST: bkl budget ok`); the allocator's hot path is O(log n).

## Red flags / decision points

- **RED:** `exec` still copies code per process until M3; with 4 GiB this is not a blocker for
  the first picture but the RSS number (R10) is measured here for M3's gate.
- **YELLOW:** superpage promotion (`vm_ops.rs`) assumes physically contiguous ranges — page
  lists must expose runs so promotion survives; a test pins it.
- **GREEN (measured):** two banks with a 2 GiB hole — the per-bank design is forced, not
  chosen.

## Definition of Done

Host tests; `just test-all` green with no fixed window; two `-m` sizes boot; the marker on
QEMU and (via B1.6) on the board's serial; the gate in `just check`; docs + CHANGELOG;
TASK-0284 closed as absorbed.
