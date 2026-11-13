---
schema: 1
title: "A thread's floating-point and vector registers"
role: guide
subsystem: kernel
sources: [source/kernel/neuron/src/core/trap/fp.rs, source/kernel/neuron/src/core/trap/fp_rule.rs, source/kernel/neuron/src/core/trap/fp_inst.rs, source/kernel/neuron/src/core/trap/fp_regs.rs, source/kernel/neuron/src/arch/riscv/trap.S, source/libs/nexus-sched/src/offers.rs, source/apps/selftest-client/src/os_lite/probes/unitsoak.rs]
contracts: [RFC-0119, ADR-0099, ADR-0048]
verified: 2026-11-13
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# A thread's floating-point and vector registers

> **Purpose** — how the integer-only kernel keeps each thread's `f0`–`f31` and `v0`–`v31` with their control state: when a state exists, when it is written, when it is loaded, and how a program learns which units the machine has.
> **Owns** — `source/kernel/neuron/src/core/trap/fp_rule.rs` and `fp_inst.rs` (the rules and the instruction decode, pure), `fp.rs` (the states and the two hooks), `fp_regs.rs` (the only routines that touch the registers), the two calls in `source/kernel/neuron/src/arch/riscv/trap.S`, `source/libs/nexus-sched/src/offers.rs` (the answer's word)
> **Contracts** — [RFC-0119](../rfcs/RFC-0119-a-thread-s-floating-point-and-vector-registers-are-its-own-kept-from-first-use.md), [ADR-0099](../adr/0099-every-userspace-program-is-built-for-one-hard-float-target-vectors-are-asked-for-at-run-time.md)
> **Invariants** — a thread without a state of a unit never runs with that unit on; a thread never runs on registers another thread or an older save left on the hart; both units are off while the kernel runs; the vector unit is offered only when every hart's registers have one width.
> **Read next** — [README.md](README.md), [smp.md](smp.md)

## The rules

The kernel is integer-only. A thread's registers of each unit — floating point, vectors — are
its own and kept lazily; the rule is one, applied once per unit (`fp_rule::Kind`).

- **No state until first use.** A thread starts with both units off. Its first instruction of
  a unit is an illegal-instruction trap: `fp::first_use` decodes which unit it needs
  (`fp_inst::asks_for`), gives the task a state of zeros and the instruction runs again. A
  vector instruction on floats needs both: one trap each.
- **Two hooks every path passes.** `trap.S` calls `__fp_trap_enter` before the handler — a
  unit's registers are written to the state only when the hardware says they changed — and
  `__fp_user_return` after it; the idle loop's staged resume calls the same return hook.
  `TaskTable::set_current` tells the hart whose states are current.
- **Loaded only when needed.** A state carries a token that is new after every write; a hart
  remembers the token it holds. Equal tokens: nothing is loaded. A thread that comes back to
  another hart, or after another thread used the unit, is loaded.
- **Kept across syscalls**, both units — one rule, and a syscall inside a vector loop loses
  nothing.
- **One vector configuration in the kernel.** At the trap entry of a thread with a vector
  state the hart's length, type and start index are set as after reset (`__vector_rest`); at
  its return they come back from its state (`__vector_resume`, or with the whole load). The
  emulator translates code per configuration — left on the hart, a thread's setting made
  every kernel path run its first time again.
- **Off in the kernel.** A hart turns both units off at bring-up and at every trap entry; one
  is on only inside the routines of `fp_regs.rs`. A unit found on at a return to user mode, or
  on for a thread without a state, is counted and printed (`KSELFTEST: fp state FAIL (`,
  `KSELFTEST: vector state FAIL (`).

## What the machine offers

| unit | offered when | a state is |
|---|---|---|
| floating point | every hart's node states F and D | 33 words: `f0`–`f31`, `fcsr` |
| vectors | every hart's node states V, floats are offered, and every hart reads the same register width (`vlenb`), a power of two from 16 to 256 bytes | 4 words (`vstart`, `vcsr`, `vl`, `vtype`) and 32 registers of that width |

The tree says which harts have a unit; the width is read from each hart's own register — the
boot hart when the tree is read (`fp::offer`), every other hart at its start
(`fp::hart_starts`). A hart with another width withdraws the vector offer for the machine
before any thread runs: `KINIT: vector unit not offered (`. What was decided is one line:
`KINIT: units offered (float= vector= vector_bytes=)`.

A program asks: `nexus_abi::sched::offers()` (topology query `OFFERS`) returns one word — the
two units, Zba, Zbb, Zbs, Zfh, Zvfh, and the vector width in bytes. A bit is set only when
every CPU carries the extension and, for a unit, the kernel keeps its state. Code that wants a
vector path asks once and keeps a scalar one.

## What a round costs

| thread | at a trap entry | at the return to user mode |
|---|---|---|
| never used a unit | one test | both units set off |
| used one, registers unchanged since the last save | a test per unit (vectors: the configuration set to the reset one) | a token compare (vectors: length, type and start set again) |
| changed its registers | the unit's state stored (33 words, or 4 + 32 registers), a new token | a token compare |
| comes back after another user of the unit, or on another hart | as above | the unit's state loaded |

## The detector

`unitsoak` (`source/apps/selftest-client/src/os_lite/probes/unitsoak.rs`, register sets in
`fpsoak.rs` and `vsoak.rs`) asks `offers()` and runs one soak per unit named: two threads of
one process hold their own patterns, hand the hart over 10 000 times by fence — every round a
block and a wake, so a pin moves both — hop over every CPU of the set, then compute at once
for several ticks. Every register is checked after every round; the vector soak compares
every byte of every register, the length, the type and the control register. Lines:
`fpsoak: rounds= partner= hops= differs=0x… partner_differs=0x…`, the same for `vsoak`, and
`SELFTEST: hw probe ok (float= vector= vector_bytes= own_bytes= word=)` once the soaks the
answer started passed and the thread's own registers have the width it states.

The kernel's side, per unit: `KSELFTEST: fp state lazy ok (threads= saves= loads= kept=
refused=0)` and `KSELFTEST: vector state lazy ok (`, and with more than one hart
`… state moved ok (from= to=)`. Host: `fp_rule` (rules, rejects, random schedules, decode).

A pin places a thread only at its next wake: a soak that only yielded never left its hart
(TASK-0363 P5 carries the finding).
