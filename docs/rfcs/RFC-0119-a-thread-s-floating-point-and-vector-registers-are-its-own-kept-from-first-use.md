---
schema: 1
id: RFC-0119
title: "A thread's floating-point and vector registers are its own: kept from first use, and a program asks what its harts offer"
status: in_progress
owners: ["@kernel-team"]
created: 2026-11-12
updated: 2026-11-12
subsystem: kernel
tasks: [TASK-0362]
depends_on: [RFC-0118]
links:
  - label: "The decision and its numbers"
    target: docs/adr/0099-every-userspace-program-is-built-for-one-hard-float-target-vectors-are-asked-for-at-run-time.md
  - label: "The standing detectors"
    target: docs/adr/0048-userspace-runtime-integrity-detectors.md
  - label: "The measurement"
    target: docs/board/measurements/2026-11-12-float-and-vectors/README.md
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# RFC-0119: A thread's floating-point and vector registers are its own: kept from first use, and a program asks what its harts offer

## Why

The kernel saves a thread's integer registers and nothing else. A program that used a
floating-point or vector register would read another thread's values after the next switch, so
no program uses them and every float operation is a library call — 3.7× to 12.3× on the
rasters (ADR-0099). The hardware has the registers on every hart of both machines we run.

## Scope

- **Owns:** what a thread may assume about its floating-point and vector registers, when the
  kernel keeps them, what a first use costs, what a program learns about its harts' instruction
  set.
- **Does not own:** the userspace build target (ADR-0099, TASK-0362 P3), the vector kernels of
  a consumer (TASK-0367 P4), the scheduler.

## Contract (normative)

### What a thread may assume

1. A thread's floating-point registers (`f0`–`f31`, `fcsr`) hold what it last wrote, across
   every syscall, interrupt, preemption and migration to another hart.
2. A thread starts with none: its first floating-point instruction traps, the kernel gives it a
   state of all zeros (rounding to nearest, no flags) and the instruction runs again. A thread
   never reads a value another thread wrote.
3. On a machine whose harts do not all carry F and D, a floating-point instruction is an
   illegal instruction, as it is today.
4. A new thread and a new process start without a state, whatever their creator holds.

### When the kernel keeps them

- A thread without a state costs nothing: no memory, no save, no restore. Its trap frame says
  the unit is off, and the kernel sets it off again at every return to user mode — whatever
  the frame said.
- A thread with a state: its registers are written to its state when it traps having changed
  them (the hardware says so), and never otherwise. Before it runs, the hart's registers are
  loaded from its state unless they still hold exactly that state — the state carries a token
  that is new after every write, a hart remembers the token it holds.
- The state belongs to the task and dies with it.
- The kernel itself is built without floating-point instructions; no kernel path reads or
  writes the registers except the two that move a state.
- The unit is off while the kernel runs: a hart turns it off at its bring-up (whatever the
  firmware left in the status word) and at every trap entry, once the save is decided; it is
  on only inside the two routines that move a state. The thread's own switch lives in its
  trap frame. A floating-point instruction that got into the kernel therefore traps instead
  of changing the registers of whichever thread the hart holds — and the kernel's code runs
  under one unit state, not under whichever the trapped thread had.

### The vector registers (TASK-0362 P2)

The same rules with the vector unit's own switch and a state of 32 registers of the harts'
width, allocated at the first vector instruction. Decided with P2: where the width comes from
when the tree does not state it, and what a thread holds across a syscall (the calling
convention makes every vector register caller-saved).

### What a program learns (TASK-0362 P2)

A program asks the kernel what its harts offer instead of assuming it: the extensions every
hart that runs threads carries, and the vector registers' width. An extension only some harts
carry is not reported. The operation's number and wire form are decided with P2 (an operation
of the sched syscall's topology family, RFC-0118, or one of its own); it reads no user memory.

## Invariants

| Invariant | Proof |
|---|---|
| a thread without a state never runs with the unit on | `test_reject_unit_on_without_a_state` |
| a thread never runs on registers another thread or an older save left on the hart | `test_reject_stale_registers_after_a_migration`, `random_schedules_keep_every_threads_registers` |
| a save happens only for registers that changed, a load only when the hart holds something else | `a_thread_that_only_reads_is_saved_once`, `a_thread_alone_on_its_hart_is_never_reloaded` |
| a changed unit without a state is refused and counted, never saved to nowhere | `test_reject_changed_registers_without_a_state` |
| an instruction that is no floating-point instruction gives no state | `test_reject_first_use_by_another_instruction` |
| the unit is off while the kernel runs | checked at every return to user mode: a unit found on counts as a refusal — `KSELFTEST: fp state FAIL (… on in the kernel)`, and `fp state lazy ok` is not printed |
| the registers survive 10 000 switches against a second thread using them, and a migration | `SELFTEST: fpsoak ok`, `KSELFTEST: fp state lazy ok`, `KSELFTEST: fp state moved ok` (multi-hart lanes, the board) |

## Failure model

- No memory for a state at first use: the thread's process is killed like any other fault it
  cannot continue from; one line names it.
- The unit found on for a thread without a state, or changed registers without a state: the
  unit is set off, the event is counted, `KSELFTEST: fp state FAIL` is printed once. The thread
  continues without the registers.
- An illegal instruction with the unit already on is an illegal instruction.
- The unit found on at a return to user mode although the kernel ran since the trap entry:
  counted and printed like the refusal above, and turned off again.

## Security

The registers are a channel between threads if they are not switched: rule 2 and the first
two invariants close it. The state is kernel memory a thread never addresses; the trap frame's
unit bits are not trusted (the kernel derives them from whether the task has a state). The
probe reports nothing a program could not find by executing an instruction and trapping.

## Proof

```bash
just test-kernel                      # the rules and the random schedules (host)
just ci-os-smp1 && just ci-os-smp     # fpsoak, the lazy line; on two harts the moved line
```

Board: `SELFTEST: fpsoak ok`, `KSELFTEST: fp state lazy ok`, `KSELFTEST: fp state moved ok`,
`[PASS] board-visible`.

## Alternatives considered

- **Let the kernel run under the trapped thread's unit state** (turn nothing off at the
  entry). Rejected, measured 2026-11-12: a stray floating-point instruction in the kernel
  would compute on a thread's registers unseen; and on the emulator with eight harts the
  kernel's paths ran about a hundred times slower the first time they were entered under a
  new unit state — one timer tick held the kernel lock 12–18 ms, every other hart queued
  behind it, and the lock budget was red in 5 of 5 runs. With the unit off in the kernel:
  0 waits above 10 ms in 3 of 3 runs (`build/logs/smp--2026-11-12T14-53-48` and the two after).
- **Save and restore at every switch, for every thread.** Rejected: 264 bytes moved twice per
  switch for threads that never touch the unit — most of the system's services.
- **Keep the registers on the hart until another thread wants them** (the classic lazy
  switch). Rejected: a thread that migrates leaves its registers on another hart, which then
  has to be interrupted to give them up.
- **A save at each of the places a hart changes its task.** Rejected: there are 25, and a
  missed one is a silent leak. The save sits at the trap entry, the load at the return to user
  mode — two places every path passes.
- **Decode nothing and enable on any illegal instruction.** Kept as the fallback for hardware
  that reports no instruction bits; with the bits, an instruction that is not the unit's gives
  no state.
