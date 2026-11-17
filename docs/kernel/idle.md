---
schema: 1
title: "An idle hart"
role: reference
subsystem: kernel
sources: [source/kernel/neuron/src/core/cpu_main.rs, source/kernel/neuron/src/core/trap/runtime.rs, source/kernel/neuron/src/core/trap/handler.rs, source/kernel/neuron/src/core/trap/baseline.rs, source/kernel/neuron/src/core/smp/runtime.rs, source/kernel/neuron/src/mm/pressure_endpoint.rs, source/kernel/neuron/src/task/wake.rs]
contracts: [ADR-0052, RFC-0021]
verified: 2026-11-17
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# An idle hart

> **Purpose** — what wakes a hart that has no task, and how often its timer fires for nothing.
> **Owns** — the idle branch of `cpu_main` and `trap::idle_arm`
> **Contracts** — [ADR-0052](../adr/0052-per-hart-earliest-deadline-timer-and-affinity-respecting-steal.md), [RFC-0021](../rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md)
> **Invariants** — no periodic tick while idle. Writing the compare register never drops a deadline that is due. The safety net is never written to the deadline shadow.
> **Read next** — [smp.md](smp.md), [README.md](README.md)

## What wakes it

A hart without a task sleeps in WFI (TASK-0363 P4). It wakes for:

- **its own deadline** — the earliest one armed on it (ADR-0052's per-hart shadow: a timed IPC,
  waitset or fence wait, a timer cap);
- **a task on its queue** — the wake hint and IPI of `request_resched`;
- **a device line** — the external interrupt, a stashed line;
- **work to steal** — a wake that queues a task behind a busy home hart kicks the lowest idle hart
  its affinity allows (`task::wake::kick_idle_thief`); a sleeping hart does not poll for it;
- **the safety net** — `IDLE_SAFETY_NS` (500 ms) after it fell asleep, for a wake nobody sent.

The boot hart sleeps too. Its net is the earliest of what its idle delivery serves: the global
deadline gate, a timer cap, the 500 ms heartbeat. Memory pressure and the swap stage's notices
wake it as events (`mm::pressure_endpoint::kick`).

## Arming before WFI

With interrupts masked, `trap::idle_arm` writes the compare register to the hart's pending shadow,
else the net — never the preemption tick of the task it last ran (that tick fired into the idle
loop once per run burst). A shadow below both deadline gates (`budgets::NEXT_DEADLINE_NS`,
`timer::next_cap_ns`, lower bounds kept under the kernel lock) is a deadline whose waiter an event
woke first, and is dropped. When a look became due between the idle check and WFI, it arms "now":
writing the register to the future would clear a pending timer interrupt and the deadline would
wait for the net. The S-mode timer trap re-arms only the net; the loop delivers what is due.

## The count

`KINIT: idle timer wakes (window_ms= total= empty= net= stale= max_empty= at=cpuN max_stray=
steals=)`, per steady window (`trap/baseline.rs`): the timer interrupts that woke a sleeping hart,
those that found nothing due, of those the nets and the `stale` ones (a deadline already served by
another hart, or no longer waited for); `max_empty` the busiest hart's empty wakes, `max_stray`
the most non-net ones on one hart; and the window's steals.

Board (`board--2026-11-17T11-06-45`, 8 harts): an idle system wakes only for its net, 119 of 119
empty wakes, 20 per hart in 10 s. Under use 98-113 empty wakes over 8 harts in 10 s (332-487 when
the last task's tick fired into the idle loop, about 1 000 per hart with the 10 ms tick); the
stray ones are stale, none a slipped tick. They come from the boot hart's global deadline arming
duplicating a hart's own, and from waits cancelled while a hart sleeps — per-hart deadline sets
remove them.
