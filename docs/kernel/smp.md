---
schema: 1
title: "SMP and IPIs"
role: guide
subsystem: kernel
sources: [source/kernel/neuron/src/core/smp, source/kernel/neuron/src/sync, source/kernel/neuron/src/core/cpu_main.rs, source/kernel/neuron/src/core/trap/runtime.rs]
contracts: [RFC-0021, RFC-0118, ADR-0049, ADR-0052]
verified: 2026-11-13
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# SMP and IPIs

> **Purpose** — how NEURON runs on several harts: bring-up, identity, the lock model, rescheduling and TLB shootdown, and which IPIs may be dropped.
> **Owns** — `source/kernel/neuron/src/core/smp/`, `source/kernel/neuron/src/sync/`
> **Contracts** — [RFC-0021](../rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md), [ADR-0049](../adr/0049-bkl-lockclass-and-softrt-cpu-placement.md), [ADR-0052](../adr/0052-per-hart-earliest-deadline-timer-and-affinity-respecting-steal.md)
> **Invariants** — a correctness IPI is never dropped. No lock is held across a context switch. Never take the BKL while holding a lock below it. A hart without a task runs on the kernel's own space; a space a hart runs on is never freed.
> **Read next** — [README.md](README.md), [topology.md](topology.md), [hart-spaces.md](hart-spaces.md), [../architecture/concurrency.md](../architecture/concurrency.md)

## Shape

- The harts are the device tree's (RFC-0118): [topology.md](topology.md).
- The interactive launcher boots `SMP=4`; proof lanes default to `SMP=1`; the `smp` profile and
  `just ci-os-smp` run several harts under MTTCG. With `SMP=1` everything lands on CPU 0 in the
  same order as a single-CPU scheduler.
- Every hart runs `cpu_main` (`source/kernel/neuron/src/core/cpu_main.rs`) and schedules from its
  own run-queue set inside the one `Scheduler`. Secondaries park in WFI until `runtime_ready()`
  (set after kernel selftests and the init spawn).

## Bring-up

`source/kernel/neuron/src/core/smp/bringup.rs` starts secondaries one by one over SBI HSM, waits for
each to settle, re-issues the start for a hart that did not come online (bounded), and prints
the gate ([topology.md](topology.md)):

- `KINIT: cpuN online` and `KGATE: smp cpuN hart=… online= asm= stage= hsm=` per CPU;
- `KGATE: smp bringup ok mask=0x…`, or `KGATE: smp bringup DEGRADED expected=… got=…` — a lost hart
  degrades the boot loudly instead of killing it;
- `KSELFTEST: smp online ok harts= clusters=`;
- `KINIT: isa from hart=… base= f= d= v= …` — what the tree states the harts offer, one line per
  run of harts stating the same (`source/kernel/neuron/src/core/boot_fdt.rs`);
- `KINIT: cpuN speed (loop_us= iters= left=)` — the time of one fixed two-instruction loop when
  the hart first enters its scheduling loop; the same loop runs on the stock system for the
  comparison of the clocks (`docs/board/measurements/2026-11-10-smp-baseline/hartspeed.rs`).

## CPU identity

`HartLocal` (one cache-line-aligned block per hart) holds the trap stack top, trap-prologue scratch
slots, the CPU index, a magic tag, the preemption tick counter and the staging slot for the next
context switch. `sscratch` and `tp` point at it; its field order is an ABI with
`source/kernel/neuron/src/arch/riscv/trap.S`. `cpu_current_id()` trusts `tp` when the magic matches
and otherwise falls back to the stack range, counting each fallback; selftests require the count to
stay 0.

## Locks

| primitive | use |
|---|---|
| `SpinIrqLock` (`source/kernel/neuron/src/sync/spin_irq.rs`) | the only lock allowed on a trap-reachable path; clears SIE before spinning, restores on drop; same-hart re-lock panics in debug builds |
| BKL (`KERNEL_LOCK` + `KernelGuard`, `source/kernel/neuron/src/core/trap/runtime.rs`) | every mutable borrow of kernel subsystems, held for the whole borrow |
| `PerCpu` (`source/kernel/neuron/src/sync/percpu.rs`) | a slot reachable only from its own CPU, IRQs masked; host-tested, not yet used by a subsystem |
| atomics | online/idle masks, resched flags, TLB mailboxes — leaf level |

Hierarchy (normative): BKL → per-CPU run-queue locks (ascending CPU index) → leaf atomics. Today
the run queues live inside the BKL aggregate, so the middle tier is empty.

BKL rules from ADR-0049: the lock-free class (only `nsec`; the debug writes went back under the
BKL so log lines stay whole) takes no lock, phased syscalls drop it for their
expensive middle, cpu0 (display and input) gets right of way while it waits, and a hart spinning on
the BKL keeps servicing TLB shootdown requests. Hold and wait times are budgeted in
`source/kernel/neuron/src/core/trap/budgets.rs`; a breach fails the boot gate.

Every take names its holder (`KernelGuard::acquire_as`: a syscall number, or the timer, an
IPI, an external interrupt, a fault, the scheduling loop), and the boot-end fence prints where
the lock's time went in the steady window (`source/kernel/neuron/src/core/trap/lock_share.rs`,
`baseline.rs`): `KINIT: bkl held us (sched= ipc= mm= task= caps= misc= timer= ipi= ext= fault=
idle=)`, `KINIT: bkl takes (…)` and `KINIT: bkl top sum` for the three largest holders;
`KINIT: ready latency class=…` states how long a runnable task waited for a hart, per QoS class
(`source/kernel/neuron/src/sched/ready_stats.rs`); a wait above one millisecond is attributed to
what the task's hart ran when the task became runnable — `late_idle` (no task: the wake did not
reach the hart in time), `late_lower` (a lower class it did not preempt), `late_peer` (its own
class or a higher one: placement). `KINIT: yield top total= pid=<pid>:<service id>:<yields> …`
names the three tasks that called `yield` most (`sched/yield_stats.rs`). The fence closes before
anyone uses the desktop, so the proof harness's observer prints the same lines again in 10 s
windows from `SELFTEST: input window open` on (telemetry op 6, `nexus_abi::sched::baseline_window`:
the window ending now, then a new one). The lines are read in
`docs/board/measurements/2026-11-10-smp-baseline/`. The lock's end is RFC-0102: classes with one
rank order, a budget per class, no global lock.

## Rescheduling

`request_resched(cpu)` sets a per-CPU pending flag with a swap. Only the caller that flips it from
0 sends the IPI; a second request while one is outstanding is counted as skipped, because the
consumer rescans the whole queue. The S_SOFT trap consumes the flag and acks. Proof chain:
request accepted → `send_ipi` ok → S_SOFT trap → ack, plus a forced-failure counterfactual.

Work stealing (`Scheduler::steal_into_current`) takes at most one task per attempt, never above
the thief's QoS class and never against a task's affinity (ADR-0052), at most once per
millisecond per CPU (`steal_rate_gate`). Markers: `KSELFTEST: work stealing ok`,
`KSELFTEST: steal park ok`, `KSELFTEST: test_reject_steal_above_bound ok`,
`KSELFTEST: test_reject_steal_higher_qos ok`.

## The space a hart runs on

A hart's `satp` names the space of the task current on it or, with no task, the kernel's own;
a space a hart runs on is never freed (`AddressSpaceError::HeldByHart`). The rule, the paths
that leave a hart without a task and the proof: [hart-spaces.md](hart-spaces.md).

## TLB shootdown

`source/kernel/neuron/src/core/smp/tlb.rs`, epoch-based and allocation-free:

1. The initiator flushes locally, bumps the global epoch and writes it into each online hart's
   mailbox (`fetch_max`, so concurrent shootdowns coalesce).
2. Harts parked in WFI are skipped (`source/kernel/neuron/src/core/smp/masks.rs`); they flush before
   their next user dispatch.
3. Responders flush fully from the S_SOFT trap and ack lock-free. An ack for a later epoch
   satisfies every earlier one.
4. The initiator waits up to 100 ms of timebase for all acks. A hart that does not ack is a lost-IPI
   kernel bug: `KPANIC: tlb shootdown ack timeout …`, never silent staleness.

`vm_unmap` initiates with the BKL dropped. Markers: `KSELFTEST: tlb shootdown ok`,
`KSELFTEST: tlb shootdown counterfactual ok`, `KSELFTEST: tlb shootdown skipped (smp=1)`.

## IPI classes

Not every IPI is equal; rate limiting must not create stale mappings.

| class | examples | rule |
|---|---|---|
| correctness — must not drop | TLB shootdown, ASID / page-table invalidation | may be merged or coalesced; under load apply backpressure (delay the caller, merge requests), never skip |
| best-effort — may throttle | resched, wake nudges | may be coalesced, rate-limited or dropped; the receiver rescans its queue on any delivered one |

What exists today: coalescing for both classes (pending flag, epoch mailbox) and the steal rate
gate. No per-task or global IPI budget is implemented; a future limiter applies to the best-effort
class only.
