---
schema: 1
id: TASK-0347
title: "No ambient authority over address spaces, and no kernel panic from a caller's size"
status: done
status_note: "closed 2026-11-03: ADR-0091 Accepted, CHANGELOG swept"
owner: ["@kernel-team"]
created: 2026-10-31
updated: 2026-11-03
subsystem: kernel
kind: hardening
novelty: first
started: 2026-10-31
done: 2026-11-03
dates_source: git-reconstructed
goal:
  metric: "syscalls that reach another address space or panic the kernel from a caller's input"
  baseline: "4 holes: as_map, as_create, spawn into any space or S-mode, a size or a fault panics"
  target: "0: every path refused by name or answered ENOMEM"
  instrument: "KSELFTEST: ambient authority reject ok ("
  result: "0 of 4 paths left: as_create/as_map retired, spawn own-space only, sizes answered ENOMEM/EINVAL, faults MapError::OutOfMemory; KSELFTEST: ambient authority reject ok on every ladder and the board (cycle 80)"
consumer: TASK-0348
links:
  - label: ADR
    target: docs/adr/0091-no-ambient-authority-over-address-spaces-a-task-maps-and-spawns-only-into-its-own-the-kernel-bounds-what-a-caller-sizes.md
  - label: Related
    target: "ADR-0080 (exec_vmo the only exec path), RFC-0085 (a task maps into its own space), ADR-0054 (errors keep their identity)"
  - label: "Found by"
    target: "the kernel memory map for TASK-0348/0349 (2026-10-30)"
  - label: follow-up
    target: "nexus-loader's future (no dependent left)"
phases:
  - id: P0
    story: "the four ambient-authority and panic paths are found, read end to end"
    state: done
    gate: "the findings in the ledger"
    started: 2026-10-30
    done: 2026-10-30
    evidence: "tasks/TASK-0347 § Found"
  - id: P1
    story: "no process maps into or starts a thread in another space; syscalls 9/10 retired"
    state: done
    gate: "spawn only into the caller's own space (EPERM otherwise)"
    started: 2026-10-31
    done: 2026-10-31
    evidence: "cargo test -p neuron (214 tests)"
  - id: P2
    story: "a caller's size or a fault never panics the kernel"
    state: done
    gate: "kheap test_reject_an_allocation_that_eats_into_the_reserve; MapError::OutOfMemory"
    started: 2026-10-31
    done: 2026-10-31
    evidence: "cargo test -p neuron"
  - id: P3
    story: "the reject rung on every QEMU profile and on the board"
    state: done
    gate: "KSELFTEST: ambient authority reject ok ( in every ladder and in scripts/board-test.sh"
    started: 2026-11-02
    done: 2026-11-02
    evidence: build/logs/board--2026-11-02T14-16-01
  - id: P4
    story: "closure: ledger, CHANGELOG and ADR-0091 status swept"
    state: done
    gate: "the docs sweep of CLAUDE.md's workflow"
    started: 2026-11-03
    done: 2026-11-03
    evidence: "2026-11-03: ADR-0091 Accepted (board cycle 80), CHANGELOG § Security 2026-10-31 present, ledger closed"
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
