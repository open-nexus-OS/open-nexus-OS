---
schema: 1
title: "A thread's floating-point registers"
role: guide
subsystem: kernel
sources: [source/kernel/neuron/src/core/trap/fp.rs, source/kernel/neuron/src/core/trap/fp_rule.rs, source/kernel/neuron/src/arch/riscv/trap.S, source/apps/selftest-client/src/os_lite/probes/fpsoak.rs]
contracts: [RFC-0119, ADR-0099, ADR-0048]
verified: 2026-11-12
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# A thread's floating-point registers

> **Purpose** — how the integer-only kernel keeps each thread's `f0`–`f31` and control register: when a state exists, when it is written, when it is loaded.
> **Owns** — `source/kernel/neuron/src/core/trap/fp_rule.rs` (the rules, pure), `source/kernel/neuron/src/core/trap/fp.rs` (the registers), the two hooks in `source/kernel/neuron/src/arch/riscv/trap.S`
> **Contracts** — [RFC-0119](../rfcs/RFC-0119-a-thread-s-floating-point-and-vector-registers-are-its-own-kept-from-first-use.md), [ADR-0099](../adr/0099-every-userspace-program-is-built-for-one-hard-float-target-vectors-are-asked-for-at-run-time.md)
> **Invariants** — a thread without a state never runs with the unit on; a thread never runs on registers another thread or an older save left on the hart; the unit is off while the kernel runs.
> **Read next** — [README.md](README.md), [smp.md](smp.md)

## The rules

The kernel is integer-only; a thread's floating-point registers are its own and kept lazily.

- **No state until first use.** A thread starts with the unit off. Its first floating-point
  instruction is an illegal-instruction trap: `fp::first_use` gives the task a state of zeros
  and the instruction runs again. Offered only when every hart of the tree carries F and D.
- **Two hooks every path passes.** `trap.S` calls `__fp_trap_enter` before the handler — the
  registers are written to the state only when the hardware says they changed — and
  `__fp_user_return` after it; the idle loop's staged resume calls the same return hook.
  `TaskTable::set_current` tells the hart whose state is current.
- **Loaded only when needed.** A state carries a token that is new after every write; a hart
  remembers the token it holds. Equal tokens: nothing is loaded. A thread that comes back to
  another hart, or after another thread used the unit, is loaded.
- **Off in the kernel.** A hart turns the unit off at bring-up and at every trap entry; it is
  on only inside `__fp_store` and `__fp_load`. A unit found on at a return to user mode, or on
  for a thread without a state, is counted and printed (`KSELFTEST: fp state FAIL (`).
- **Proof.** Host: `fp_rule` (rules, rejects, random schedules). OS: `SELFTEST: fpsoak ok`,
  `KSELFTEST: fp state lazy ok (threads= saves= loads= kept= refused=0)`, and with more than
  one hart `KSELFTEST: fp state moved ok (from= to=)`.

## What a round costs

| thread | at a trap entry | at the return to user mode |
|---|---|---|
| never used the unit | two tests | the unit set off |
| used it, registers unchanged since the last save | two tests | a token compare |
| changed its registers | 33 stores, a new token | a token compare |
| comes back after another user of the unit, or on another hart | as above | 33 loads |

## The detector

`fpsoak` (`source/apps/selftest-client/src/os_lite/probes/fpsoak.rs`): two threads of one
process hold their own patterns, hand the hart over 10 000 times by fence — every round a
block and a wake, so a pin moves both — hop over every CPU of the set, then compute at once
for several ticks. Every register is checked after every round. Its line:
`fpsoak: rounds= partner= hops= differs=0x… partner_differs=0x…`.

A pin places a thread only at its next wake: a soak that only yielded never left its hart
(TASK-0363 P5 carries the finding).
