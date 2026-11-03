<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0091: No ambient authority over address spaces — a task maps and spawns only into its own space; the kernel bounds what a caller sizes

- Status: Accepted (2026-11-02, board cycle 80: `KSELFTEST: ambient authority reject ok (` on the board and every QEMU ladder; proposed 2026-10-31 by TASK-0347, found while mapping the kernel for the memory model)
- Date: 2026-10-31
- Links:
  - Tasks: `tasks/TASK-0347-no-ambient-authority-over-address-spaces-and-no-kernel-panic-from-a-callers-size.md`
  - RFCs: RFC-0085 (kernel-owned VA: a task maps into its own space), RFC-0004 (W^X at the
    mapping boundary)
  - Related ADRs: ADR-0080 (a process runs from a sealed image; `exec_vmo` is the only exec
    path), ADR-0054 (map errors keep their identity across the ABI), ADR-0077 (lazy objects)

## Context

Reading the kernel for the memory model (TASK-0348/0349) turned up four ways any process could
reach past its own memory or panic the kernel — none needs a capability:

1. **`as_map` (10) mapped into any address space by a raw handle number**, at a caller-chosen VA
   with caller-chosen permissions (W^X the only check) — another process's space, or a hole in
   the kernel half, whose top-level tables every space shares. `as_create` (9) handed out fresh
   spaces (ASIDs) to anyone. Nothing in the OS uses either: processes come from `exec_vmo`
   (ADR-0080), mappings from `vm_map` into the caller's own space (RFC-0085). Their last users
   were a legacy ELF loader (`nexus-loader`'s OS mapper, execd's std backend) no build compiles.
2. **`spawn` (7) trusted a raw address-space handle**: a thread started at the caller's choice of
   entry inside another process's space; with no handle it started a **supervisor-mode** task at
   any kernel-text address with a caller-chosen global pointer.
3. **A lazy object's length had no bound**: its page words (4 B per page) come from the fixed
   8 MiB kernel heap, and every kernel allocation panics on failure — one `vmo_create(huge, LAZY)`
   was a kernel panic. An eager object's block list and the task table grow the same way.
4. **A page-table frame from an empty pool panicked** — reachable from any user fault.

## Decision

1. **`as_create` (9) and `as_map` (10) are retired** — removed from the kernel table, the ABI and
   the legacy loader; the numbers are never reused (like 4, 13, 17, 27, 50). A space comes from
   `exec_vmo` only; a task maps into its own space only.
2. **`spawn` (7) starts a thread in the caller's own space only** (`as_self`), left suspended for
   its parent; another space, or none, is `EPERM`. A supervisor task is the kernel's alone — the
   kernel's selftests start theirs through the kernel's own API.
3. **The kernel bounds every allocation a caller sizes.** No object is larger than the memory
   that could back it (the pool's frames; `EINVAL`). The kernel heap keeps a reserve
   (`kheap::RESERVE`, 1 MiB) a caller-sized allocation — a lazy object's words, an object's
   block list, the VMO table's growth, a new task — may not eat into, and those allocations are
   fallible: the caller hears `ENOMEM`, the kernel never panics.
4. **An empty pool is an error at every map**: a page-table frame that cannot be had is
   `MapError::OutOfMemory` — `ENOMEM` to a syscall, the out-of-memory kill to a fault.
5. **`vmo_create` keeps its cause across the ABI**: bad length `EINVAL`, no frame or heap
   `ENOMEM`, a full object table `ENOSPC` (it answered `EPERM` for all of them).

Out of scope here (the memory model, TASK-0349): a heap that grows, sparse lazy objects whose
size costs nothing, task slots reused, refcounts for VMOs named by in-flight capabilities.

## Consequences

- **Positive:** no capability-free path into another process or into supervisor mode; a
  caller's size can no longer stop the kernel; one exec path, one map path.
- **Negative / accepted cost:** `nexus-loader` keeps only its host-tested ELF parsing (no
  dependent left — a follow-up decides whether it stays); execd's std backend execs nothing
  (it never did in any build that runs).
- **Proof:** `KSELFTEST: ambient authority reject ok (retired=2 spawn=2 bound=1 reserve=1
  objects=N)` on every QEMU profile and on the board; host tests of the reserve rule.

## Alternatives considered

- **Keep `as_map`, bound it to the user half** — still a map into any space named by a number;
  capability-gate it — a second map path beside `vm_map` with no consumer.
- **A capability for address spaces (Fuchsia's VMAR handles)** — the right shape if a process
  ever builds another's space; ours never does (`exec_vmo` builds it in the kernel).
- **Grow the kernel heap instead of bounding** — that is TASK-0349's growable heap; a bound and a
  reserve stay necessary even then (the frame pool is finite too).
