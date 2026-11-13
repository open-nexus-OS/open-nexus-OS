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
  a consumer (the painter's fills and blends, TASK-0362 P4; glyphs, TASK-0367 P4), the
  scheduler.

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

### The vector registers

The same rules, with the vector unit's own switch and its own state — a thread that only
computes in floats never pays for vectors, and the reverse.

1. A thread's vector registers (`v0`–`v31`, every byte of their width), its vector length and
   type and the unit's control state (`vstart`, `vcsr`) hold what it last set, across every
   syscall, interrupt, preemption and migration. The calling convention would let the kernel
   discard them at a syscall; it does not, so a syscall the compiler placed in the middle of a
   vector loop costs nothing but its time.
2. The state is four words and 32 registers of the harts' width, allocated at the thread's
   first vector instruction: registers of zeros, no type set yet (`vill`), length 0 — what
   the hardware has after reset.
3. A vector instruction on floats needs both units; a thread that has neither traps once for
   each.
4. **The width is one number for the machine.** The kernel reads it from each hart's own
   register (`vlenb`), not from the tree — the boot hart when the tree is read, every other
   hart at its start, before any thread runs. The unit is offered only when every hart's node
   states V, F and D are offered, every hart reads the same width, and that width is a power
   of two from 16 to 256 bytes (128 to 2048 bits; a wider state has no measured consumer and
   is one constant, `MAX_VECTOR_BYTES`). A hart that reads another width withdraws the offer
   for the whole machine with one line, `KINIT: vector unit not offered (cpu= vector_bytes=
   boot hart …)`: a thread could not move between such harts.
5. Without the offer a vector instruction is an illegal instruction.
6. **The kernel runs under one vector configuration.** A hart's length, type and start index
   are as after reset (no type, length 0, start 0) from its bring-up and from every trap
   entry of a thread that has a vector state — the thread's own are in its state by then.
   At its return they are set from the state again, with the registers when the hart no
   longer holds them, alone when it does. A thread without a vector state never sees a
   configuration: the unit is off for it.

### What a program learns

A program asks the kernel what every CPU offers instead of assuming it: query `OFFERS` (5) of
the sched syscall's topology operation (RFC-0118), index 0. It reads no user memory and
carries no authority; any task may ask.

The answer is one word (`nexus_sched::Offers`, `nexus_abi::sched::offers()`):

| Bits | Meaning |
|---|---|
| 0 | floating point (F and D), and the kernel keeps a thread's registers |
| 1 | vectors (V), and the kernel keeps a thread's registers |
| 2, 3, 4 | the bit-manipulation extensions Zba, Zbb, Zbs |
| 5, 6 | half-precision floats, scalar (Zfh) and in vectors (Zvfh) |
| 32–47 | the width of a vector register in bytes; 0 exactly when bit 1 is 0 |

- A bit is set only when **every** CPU that runs threads carries the extension — a thread may
  run on any of them — and, for the two units, only when the kernel offers a state. An
  extension only some harts carry is not reported.
- Zfh is reported only with bit 0, Zvfh only with bit 1.
- Extensions a program cannot act on from user mode by itself (cache-block operations,
  privileged ones) are not in the word; they come with the operation that uses them.
- A reader refuses a word that contradicts itself (a width without bit 1, bit 1 without a
  width, a width no register has): `Offers::from_raw` → `Unsupported`. Bits it does not know
  are kept and mean nothing to it.
- A program that uses an extension the word does not name gets an illegal instruction.

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
| a vector instruction gives a vector state only where the unit is offered, and no other instruction does | `test_reject_vector_use_without_the_extension`, `a_vector_instruction_on_floats_asks_for_both_units_one_trap_each` |
| the kernel runs under the reset vector configuration, and a thread gets its own back | `SELFTEST: vsoak ok` compares each thread's length and type after every round — two threads with different ones on the same harts; the lock budget on eight emulated harts (`KSELFTEST: bkl budget ok`) |
| a vector state holds every register of the machine's width | `a_state_holds_every_register_of_its_width`; `SELFTEST: vsoak ok` (every byte of `v0`–`v31`, the length, the type and the control register, 10 000 hand-overs through two syscalls each), `KSELFTEST: vector state lazy ok`, `KSELFTEST: vector state moved ok` |
| the answer names an extension only if every CPU carries it, a unit only if the kernel keeps it, and never a width without the unit | `test_reject_a_width_no_register_has`, `test_reject_a_word_that_contradicts_itself` (`nexus-sched`); `SELFTEST: hw probe ok (` — the soaks ran for the units it named, and the thread's own registers have the width it stated |

## Failure model

- No memory for a state at first use: the thread's process is killed like any other fault it
  cannot continue from; one line names it.
- The unit found on for a thread without a state, or changed registers without a state: the
  unit is set off, the event is counted, `KSELFTEST: fp state FAIL` is printed once. The thread
  continues without the registers.
- An illegal instruction with the unit already on is an illegal instruction.
- A hart whose vector registers have another width than the boot hart's: the vector unit is
  not offered on this boot (one `KINIT:` line names the hart and both widths); floats stay.
- The topology query with an index other than 0, or on a kernel that does not know it: refused
  (`EINVAL`), and `offers()` reports `Unsupported` — a program then uses neither unit's
  optional paths.
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
just ci-os-smp1 && just ci-os-smp     # fpsoak, vsoak, the probe, the lazy lines; on two harts the moved lines
```

Board: `SELFTEST: fpsoak ok`, `SELFTEST: vsoak ok`, `SELFTEST: hw probe ok (`, and for both
units `KSELFTEST: … state lazy ok` and `… state moved ok`; `[PASS] board-visible`.

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
- **Leave a thread's vector length and type on the hart while the kernel runs** (they cost
  nothing with the unit off). Rejected, measured 2026-11-12: the emulator translates code per
  vector configuration whether the unit is on or not, so every kernel path ran its first time
  again under each configuration a soak thread had left — eight harts: lock budget red in 2
  of 2 runs (waits of 20.8 and 96.7 ms; one `spawn` held the lock 95 ms right after the
  soak), green in 4 of 4 before the vector soak existed. With the configuration set to the
  reset one at the trap entry: green in 3 of 3 (waits 5.7–10.1 ms, holds 3–4 ms;
  `build/logs/smp--2026-11-12T17-35-07` and the two after). On hardware the rule costs a
  vector thread one length-and-type setting per trap entry and one per return; a thread
  without the unit pays nothing.
- **Discard the vector registers at a syscall** (the calling convention allows it, and a
  blocked thread's state would not need saving). Rejected: two rules for two units, a system
  call the compiler inlines into a vector loop silently loses the loop's registers on one
  path and not on the other, and the saving is one store of a state per blocking call of a
  thread that is mid-vector — the lazy rule already skips every other one.
- **Take the vector width from the tree.** Rejected: neither of our two trees states it, and
  the register is the authority. The tree says which harts have the unit; each hart says how
  wide it is.
- **A syscall of its own for the probe, or a page of bits mapped into every process.**
  Rejected for now: the topology operation already answers "what machine is this" from
  registers alone, and one word covers every extension a program can act on today. A program
  asks once at start; nothing here is hot.
- **Decode nothing and enable on any illegal instruction.** Kept as the fallback for hardware
  that reports no instruction bits; with the bits, an instruction that is not the unit's gives
  no state.
