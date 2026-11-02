---
title: TASK-0347 No ambient authority over address spaces, and no kernel panic from a caller's size
status: In Progress (2026-10-31 — code + host tests + QEMU ladder; board cycle pending)
owner: @kernel-team
created: 2026-10-31
depends-on: []
follow-up-tasks: [TASK-0349 the memory model (growable kernel heap, sparse lazy objects, task-slot reuse, VMO refcounts that count in-flight capabilities), nexus-loader's future (no dependent left)]
links:
  - ADR: docs/adr/0091-no-ambient-authority-over-address-spaces-a-task-maps-and-spawns-only-into-its-own-the-kernel-bounds-what-a-caller-sizes.md
  - Related: ADR-0080 (exec_vmo the only exec path), RFC-0085 (a task maps into its own space), ADR-0054 (errors keep their identity)
  - Found by: the kernel memory map for TASK-0348/0349 (2026-10-30)
---

# TASK-0347 — No ambient authority over address spaces, no kernel panic from a caller's size

## Found (2026-10-30, read end to end)

Any process, with no capability at all, could:

1. **map into any address space** — `as_map` (10) took a raw space handle, a caller-chosen VA
   (the kernel half included, whose top tables every space shares) and caller-chosen
   permissions; `as_create` (9) handed out spaces/ASIDs. No OS user: processes come from
   `exec_vmo`, mappings from `vm_map` into the caller's own space.
2. **start a thread in another process's space** — `spawn` (7) trusted the raw handle — and,
   with no handle, **a supervisor-mode task** at any kernel-text address with a chosen `gp`.
3. **panic the kernel by a size** — a lazy object's page words (4 B/page) come from the fixed
   8 MiB kernel heap, every kernel allocation panics on failure, and `vmo_create` bounded no
   length; an eager object's block list and the task table grew the same way.
4. **panic the kernel by a fault** — a page-table frame from an empty pool panicked.

## Done

- **9/10 retired** (kernel table, nexus-abi wrappers, `nexus-loader`'s OS mapper, execd's std
  backend's legacy loader); numbers never reused. The kernel selftests make their spaces, maps and
  supervisor tasks through the kernel's own API (`selftest/as_setup.rs`).
- **`spawn`**: only the caller's own space (`EPERM` otherwise); every spawned thread starts
  suspended for its parent.
- **Bounds**: an object is at most the pool's frames (`EINVAL`); `kheap::RESERVE` (1 MiB) a
  caller-sized allocation may not eat into; lazy words, block lists, the VMO table's growth and a
  new task are allocated fallibly (`ENOMEM`). `vmo_create` errors keep their cause (EINVAL /
  ENOMEM / ENOSPC instead of EPERM for everything).
- **Page tables**: `MapError::OutOfMemory` — `ENOMEM` to a syscall, the out-of-memory kill to a
  fault (`FillError::Exhausted`), never a panic.

## Proof

- Host: `cargo test -p neuron` (214 tests incl. `kheap::test_reject_an_allocation_that_eats_into_the_reserve`).
- QEMU (every profile's ladder): `KSELFTEST: ambient authority reject ok (retired=2 spawn=2
  bound=1 reserve=1 objects=N)` right behind `KSELFTEST: w^x enforced`; `as map ok`, `w^x
  enforced`, `exit ok`, `wait ok` still green through the kernel API.
- Board: the same marker in `scripts/board-test.sh` (the board's own heap and pool: objects=1).

## Not doing here

A heap that grows, sparse lazy objects, task-slot reuse, refcounts that count capabilities in
flight — the memory model (TASK-0349). Whether `nexus-loader` (host ELF parsing, no dependent
left) stays is the operator's call.
