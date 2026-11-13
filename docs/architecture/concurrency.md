---
schema: 1
title: "Concurrency model"
role: guide
subsystem: kernel
sources: [source/kernel/neuron/src/sync, source/kernel/neuron/src/core/trap/runtime.rs, source/kernel/neuron/src/core/trap/budgets.rs, source/kernel/neuron/src/core/smp, source/libs/nexus-sync]
contracts: [RFC-0021, ADR-0049]
verified: 2026-11-13
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Concurrency model

> **Purpose** — how the kernel shares state across harts, the rules every change must keep, and the anti-patterns that have cost real boots.
> **Owns** — `source/kernel/neuron/src/sync/`, the BKL in `source/kernel/neuron/src/core/trap/runtime.rs`
> **Contracts** — [RFC-0021](../rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md), [ADR-0049](../adr/0049-bkl-lockclass-and-softrt-cpu-placement.md)
> **Invariants** — acquire locks strictly downward; no guard across a context switch; every wait ends in a decision on a path the missing signal cannot starve.
> **Read next** — [../kernel/smp.md](../kernel/smp.md), [../standards/RUST_STANDARDS.md](../standards/RUST_STANDARDS.md)

## The model in one paragraph

The kernel is a **measured, budgeted big kernel lock** with three declarative escape classes
(lock-free, phased, cpu0 right of way; ADR-0049). Each hart's run queues sit behind that hart's own
lock, below the BKL (TASK-0363 P2; their callers still hold the BKL); leaf state (online mask, resched flags, TLB mailboxes, counters) is atomics. It is not a
lock-free kernel: IPC, present and timer arming run under the BKL, and splitting the lock is not
done. Hold and wait times are budgeted in `source/kernel/neuron/src/core/trap/budgets.rs` and
enforced by the boot gate (`KSELFTEST: bkl budget ok`). Every take names its holder and the
boot-end fence prints the lock's time per subsystem ([../kernel/smp.md](../kernel/smp.md)); the
lock's end — classes with one rank order, no global lock — is RFC-0102.

## Ownership

- Kernel subsystems are `!Send`/`!Sync` by assertion (`static_assertions::assert_not_impl_any!`):
  `Scheduler`, `TaskTable`, `Router`, `CapTable`, `AddressSpaceManager`, `AddressSpace`,
  `PageTable`. Moving one across harts is a compile error, not a convention.
- `KernelHandles` (`source/kernel/neuron/src/core/trap/runtime.rs`) is the one `Send + Sync` door:
  raw handles to scheduler, tasks, router, spaces, timers, waitsets and fences, materialized
  mutably only through `KernelGuard`, which holds the BKL for the whole borrow.
- Hart-local state (`HartLocal`, `source/kernel/neuron/src/core/smp/mod.rs`) is written only by its
  own hart, or by the boot hart strictly before that hart starts.
- Newtypes carry identity and construction authority (`source/kernel/neuron/src/types.rs`):
  `Pid` from `TaskTable`, `Asid` from `AddressSpaceManager`, `AsHandle` from the kernel only,
  `HartId` vs `CpuId` kept apart.

## Lock hierarchy (normative)

| tier | lock | guards |
|---|---|---|
| 1 | `KERNEL_LOCK` (BKL, a `SpinIrqLock`) via `KernelGuard` | tasks, the order of scheduling steps, IPC router, address spaces and ASIDs, timers, waitsets, fences |
| 2 | one hart's run-queue lock (`sched::RUN_QUEUES`), held for one queue operation; a second hart's only by try-lock (a steal, its park) | that hart's queue per class and its running task (`sched/run_queue.rs`, host-tested) |
| 3 | leaf atomics and `SpinIrqLock` leaves (console line, console ring, device lines `irq::LINES`) | masks, counters, mailboxes, console bytes, each line's controller context and its enable writes |

Rules:

- Acquire strictly downward. Never take the BKL while holding a lock below it.
- `SpinIrqLock` (`source/kernel/neuron/src/sync/spin_irq.rs`) is the only lock allowed on a
  trap-reachable path: SIE is cleared before the spin, so a trap on the holding hart cannot re-enter.
- No guard is held across a context switch. The schedule decision stages the next frame into
  `HartLocal.resume_frame` under the BKL, drops the guard, then switches (`cpu_main`,
  `source/kernel/neuron/src/core/cpu_main.rs`).
- Fault diagnostics on the way to `panic!` read handles lock-free
  (`runtime_kernel_handles_diagnostic`) and never mutate — a fault inside a syscall already holds
  the BKL.
- A hart spinning for the BKL keeps servicing TLB shootdowns, so a BKL-holding initiator can
  always collect its acks ([smp.md](../kernel/smp.md)).

## Atomic orderings

| ordering | use |
|---|---|
| `Relaxed` | counters and statistics |
| `Acquire` / `Release` | flags that publish data (online mask, resched pending, mailbox epochs) |
| `AcqRel` swap / `fetch_max` | claim-or-coalesce (one IPI per outstanding resched, shootdown epochs) |
| `SeqCst` | only with a written reason |

Every `unsafe impl Send/Sync` carries a `SAFETY` comment naming the invariant and what enforces it.

## Userspace

Services are no_std processes; shared-memory locking is the exception. `source/libs/nexus-sync/`
provides a plain `SpinLock` (Acquire/Release, non-reentrant). Cross-process data moves by IPC and
VMOs, not shared mutable state. `parking_lot` is forbidden in the OS graph (RFC-0009).

## Anti-patterns

### 1. Unsynchronized global state

`static mut` written from more than one hart is a data race. Use an atomic, a `SpinIrqLock`, or
state behind the BKL.

### 2. Unbounded queues

Every kernel queue has a capacity and a deterministic reject on saturation (scheduler QoS queues,
IPC budgets). Growth or unbounded retry is a DoS vector.

### 3. The wrong lock in trap context

A lock that does not mask interrupts, taken on a path a trap can reach, deadlocks the hart the
first time the trap lands while it is held.

### 4. O(tasks) under the BKL on a per-transition path

The BKL is global, so work that is cheap per call becomes the machine's ceiling when it runs on every
scheduling transition. The deadline sweep (`wake_expired_blocked` in
`source/kernel/neuron/src/syscall/api/mod.rs`) once held the BKL for seconds per boot. The fix is
the shape to copy: an O(1) gate (`NEXT_DEADLINE_NS` in `budgets.rs`, the earliest armed deadline,
lowered when a task blocks and recomputed by each scan that runs) in front of the scan. When adding
a per-transition hook, publish a bound that lets it be skipped.

### 5. A deadline on the path it bounds

The most expensive bug class in the boot path is not a missing timeout but a timeout that cannot
run in the case it exists for:

```rust
// BAD: the reveal is bounded... inside the present handler.
fn on_present(&mut self) {
    let elapsed = now() - self.first_content_ns;
    if self.plane0 && (self.cursor_ready || elapsed > FALLBACK) { self.reveal(); }
}
// If the peer stops presenting (it lost the ack that returns its credits),
// this never runs again: no present, no cap.

// BAD: a credit that is promised but never expires.
if self.frames_in_flight >= MAX_IN_FLIGHT { return false; } // forever, if an ack is lost
```

Two normative rules:

1. **Every wait terminates in a decision** — ready or explicitly degraded, never "still waiting".
   A credit is a lease: when it expires, proceed loudly rather than freeze. A one-shot precondition
   (attach, cursor upload, content rect) is state to re-drive, not an event to hope for.
2. **The deadline is evaluated on a path the bounded thing cannot starve** — a timer, a pacer or a
   supervisor, never the handler the missing signal suppresses.

Observed instances, all the same shape: windowd waiting for an attach that had already been
consumed; app-host waiting for a content rect and then inventing a desktop size; gpud holding the
splash because its bounded reveal sat inside the present path while windowd had stopped presenting
on lost credits.

## How it is tested

| layer | proof |
|---|---|
| compile time | `assert_not_impl_any!` / `assert_impl_all!` on the types above |
| host | `just test-kernel` — `PerCpu`, `SpinIrqLock` and the pure SMP logic |
| QEMU | `KSELFTEST: kernel lock contention bounded ok` (lock ping: exact count across harts), `KSELFTEST: smp online ok`, `KSELFTEST: tlb shootdown ok`, `KSELFTEST: bkl budget ok (…)`; lanes `just test-os smp` and `just ci-os-smp` |
