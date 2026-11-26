---
schema: 1
title: "An idle hart"
role: reference
subsystem: kernel
sources: [source/kernel/neuron/src/core/idle.rs, source/kernel/neuron/src/core/cpu_main.rs, source/kernel/neuron/src/core/trap/own_deadlines.rs, source/kernel/neuron/src/core/trap/runtime.rs, source/kernel/neuron/src/core/trap/baseline.rs, source/kernel/neuron/src/core/smp/runtime.rs, source/kernel/neuron/src/task/deadline_rule.rs, source/kernel/neuron/src/timer.rs, source/kernel/neuron/src/mm/pressure_endpoint.rs, source/kernel/neuron/src/task/wake.rs]
contracts: [ADR-0052, RFC-0021]
verified: 2026-11-26
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# An idle hart

> **Purpose** — what wakes a hart that has no task, and how often its timer fires for nothing.
> **Owns** — `source/kernel/neuron/src/core/idle.rs`, `trap/own_deadlines.rs`, `trap::idle_arm`
> **Contracts** — [ADR-0052](../adr/0052-per-hart-earliest-deadline-timer-and-affinity-respecting-steal.md), [RFC-0021](../rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md)
> **Invariants** — no periodic tick while idle. A hart arms only its own deadlines. Writing the compare register never drops a deadline that is due. An arm never pushes a running task's preemption tick out, and a dispatch never pushes a due own deadline out. The safety net is never written to the deadline shadow.
> **Read next** — [smp.md](smp.md), [README.md](README.md)

## What wakes it

A hart without a task sleeps in WFI (`core/idle.rs`, TASK-0363 P4). It wakes for:

- **its own deadline** — of a task that blocked on it (a timed receive, send, waitset or fence
  wait) or of a timer cap armed on it (TASK-0363 P8);
- **a task on its queue** — the wake hint and IPI of `request_resched`; a wake that would queue
  a task behind a peer at its home places it on a free hart instead (`placement`, TASK-0363 P5:
  [smp.md](smp.md)) — a sleeping hart does not poll for work;
- **a device line** — the external interrupt, a stashed line;
- **the safety net** — `IDLE_SAFETY_NS` (500 ms) after it fell asleep, for a wake nobody sent.

The boot hart sleeps too; its net is the 500 ms heartbeat of its idle delivery. Memory pressure
and the swap stage's notices wake it as events (`mm::pressure_endpoint::kick`). It is no
backstop for the other harts' deadlines.

## Own deadlines

A deadline belongs to the hart it was armed on: the hart that blocked the task, or ran
`timer_set` (`Task::blocked_on`, `TimerState::arm_cpu`), armed its timer for it (`arm_wakeup`,
ADR-0052) — wherever the task runs later. `trap/own_deadlines.rs` keeps each hart's floor: the
earliest deadline of its blocked tasks and of its timer caps, a lower bound lowered by the block
or the arm and set exact by the deadline walk and every timer pop or cancel. A thread blocks under
no kernel lock, beside a walk, so each bound is two words (`sync/noted_bound.rs`): what waiters
noted since the last walk began, and that walk's result — read as their minimum, a read beside a
walk made again after it. A walk counts a wait still being prepared, and the block lowers its
bound in the hold that makes the wait visible: no deadline is in neither word. A secondary with only timer caps due delivers them under the timer table's own lock, without
the kernel lock (`caps_due_alone`, TASK-0330 P4). A hart that delivered re-arms to its own floor (`trap/timer_events.rs`); before, it
re-armed to the global earliest — another hart's deadline, which then woke two harts, the second
for nothing.

## Arming before WFI

With interrupts masked, `idle::sleep` arms `deadline_rule::sleep_arm` (host-tested): the own
floor when it comes before the net, else the net — never the preemption tick of the task it last
ran (that tick fired into the idle loop once per run burst). The floor is held against the global
gates (`trap::own_floor_ns`: the larger of two lower bounds is one too), so an own floor left low
by a cancelled wait is never due while nothing is due globally. When a look became due between the
idle check and WFI, it arms "now": writing the register to the future would clear a pending timer
interrupt. The S-mode timer trap re-arms only the net; the loop delivers what is due.

While a user task runs, the same register carries its preemption tick: `arm_wakeup` writes the
earlier of the deadline and the 10 ms tick (`hal::platform::PREEMPT_TICK_NS`), the shadow keeps the
deadline. A re-arm at an own deadline 1 s out left a task unpreempted and an interactive peer
waiting 25 ms; now a same-class wait ends at the slice (board max 11.5 ms).

A dispatch from the loop arms the tick the same way and never later than the hart's own floor:
"now" for a deadline already due, with interrupts masked up to the user code, where it fires and
its handler delivers it (`trap::dispatch_tick_arm`, `deadline_rule::dispatch_arm`). Until
2026-11-26 every switch wrote the tick and left a due deadline out: one hart switching between two
tasks of an IPC exchange faster than the tick took no timer interrupt for 6 s — no timer cap, no
deadline (headless, `SELFTEST: ingress rate FAIL`). A periodic timer delivered late skips the
periods that passed and says how many (`missed`, `deadline_rule::periodic_rearm`).

## The count

`KINIT: idle timer wakes (window_ms= total= empty= net= stale= dry= max_empty= at=cpuN max_stray=
steals=)`, per steady window (`trap/baseline.rs`): the timer interrupts that woke a sleeping hart,
those that found nothing due, of those the nets and the `stale` ones (an armed deadline no longer
waited for); `dry` the idle walks that found a deadline due and woke nothing (a kernel-lock take
for nothing, counted in `empty`); `max_empty` the busiest hart's empty wakes, `max_stray` the most
non-net ones on one hart; and the window's steals.

Board, 8 harts: with the 10 ms tick about 1 000 empty wakes a hart in 10 s; with the last task's
tick firing into the idle loop 332-487 over all harts; with the boot hart's global backstop
(`board--2026-11-17T11-06-45`) 98-113, max 17-29 a hart, the strays all stale. With own deadlines
(`board--2026-11-17T12-09-56`): an idle system wakes only for its net (20 a hart); under use
max_empty 15-20 a hart, stale 0-11 over all harts, dry 0.
