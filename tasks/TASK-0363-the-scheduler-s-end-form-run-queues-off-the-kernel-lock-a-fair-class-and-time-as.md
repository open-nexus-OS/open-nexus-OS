---
schema: 1
id: TASK-0363
title: "The scheduler's end form: run queues off the kernel lock, a fair class, and time as a right — a budget per period, delegated as a subset, revoked in bounded time"
status: in_progress
owner: ["@kernel-team"]
created: 2026-11-10
updated: 2026-11-18
subsystem: kernel
kind: service
novelty: first
estimate: 5
started: 2026-11-11
dates_source: recorded
goal:
  metric: "wake-to-run latency p99 of a thread holding a time right on the board; run-queue operations under the kernel lock; conservation of delegated budgets"
  baseline: "every run queue lives inside the kernel lock; strict classes with a 10 ms tick; a class is a label any service may claim; wakes ran 0.3-5.1 ms late before ADR-0096 (board cycle 83); today's histogram is TASK-0330/P0's"
  target: "wake-to-run p99 <= 200 us for a time-right holder with 8 harts loaded; 0 run-queue operations under the kernel lock; the sum of delegated budgets never above the root in 10 000 random delegate, steal and migrate steps of the host model"
  instrument: "KSELFTEST: sched wake latency line; the kernel lock's holder table; sched host tests"
consumer: TASK-0365/P1
phases:
  - id: P1
    story: "the contract is decided on a host model before kernel code: a time right is a capability holding a budget per period, it is delegated only as a subset, revoked in bounded time, admitted against the harts' capacity, and what a steal and a migration do to it is stated"
    state: done
    gate: "the scheduler RFC seeded with its three lenses; host model: test_reject_delegate_beyond_held, test_reject_admit_beyond_capacity, test_reject_run_after_revoke, conservation over 10 000 random steps; TRACK-TIME-AS-RESOURCE gate G3 answered in the RFC"
    started: 2026-11-13
    done: 2026-11-13
    evidence: "source/libs/nexus-sched/tests/time_right.rs (13 host tests green, commit be98f9ed); docs/rfcs/RFC-0120"
    note: "built 2026-11-13: RFC-0120 seeded (the contract, the three rules for a steal and a migration = the track's gate G3, revoke in one step, G1 and G2 stated); the pure core nexus-sched::time_right (no allocation, no lock) with 13 host tests green: test_reject_delegate_beyond_held, test_reject_admit_beyond_capacity, test_reject_run_after_revoke, test_reject_migrate_beyond_the_targets_capacity, conservation over 10 000 random steps (every operation exercised both ways; audit after each; no reservation above share x time + one budget). Open for P3: capacity (80 % in the tests), table sizes, the syscall surface"
  - id: P2
    story: "each hart schedules under its own lock: a wake, a steal and a switch on one hart never wait for a syscall on another"
    state: done
    gate: "the sched tests run on the host (today they run nowhere); QEMU smp lane green; board: no run-queue operation among the kernel lock's holders"
    started: 2026-11-13
    done: 2026-11-16
    evidence: build/logs/board--2026-11-16T12-40-08
    note: "opened 2026-11-13. S1 (5e3efd33): per-hart run-queue leaf locks, host-tested core. S2 (committed after board cycle 24): a task's frame in its scheduling record at a fixed address, the owner word (Free/Queued/Running) handed on under the queue lock, every frame access checked (KINIT: FAIL frame owner) — it found the preemption's late save and yield's early requeue, both fixed. S3 BUILT 2026-11-16 (ADR-0100, Proposed): the idle loop picks from its own queue and starts the task without the kernel lock (core/dispatch.rs::dispatch_unlocked — frame, floating-point states and space view from the record; set_runs_here outside the task table; enter_space shared with the locked path; HartSpaces atomic and installed); a task the record cannot start goes to the locked path already picked; kill and freeze withdraw under the holding hart's queue lock (Scheduler::withdraw) instead of running_on + purge; the floating-point states moved into the record (fp_if_held for set_current: the kernel selftests act as queued tasks). QEMU 1, 2, 8 harts (8 harts once red by its 90 s budget cutting pinched's third restart, green at 120 s) and usb-visible 32/32: 0 breaches, steal park ok. Gate left (ADR-0098: a step that does not move its holder's share is reverted): board — the idle loop's share of the kernel lock (KINIT: bkl held us idle=, 3.3 s per 10 s window before) falls; no run-queue operation of a dispatch among the lock's holders; ladder 118 BOARD 2026-11-16 (build/logs/board--2026-11-16T12-16-50 S3, build/logs/board--2026-11-16T12-40-08 S3 with KINIT: bkl idle split): ladder 118, 0 breaches, no panic, bkl budget max_wait 399 us (5-6 ms before S2), wake ipi max 44 us. The idle group's lock time did not fall (3.3-3.8 s a window) — the split names why: dispatch 0 us/0 takes (the unlocked path starts every task), steal 0.03-0.14 s, the boot hart's delivery 3.4-4.7 s (events 1.6-2.2 s, the deadline walk over every task 1.4-2.0 s): the boot hart spins and polls under the lock each turn. That is P6's (one deadline walk, no walk per turn) and P4's (the boot hart sleeps until an event) — the phase's board gate (no run-queue operation of a dispatch among the holders) is met; ADR-0098's holder rule applies to the delivery, which this step does not touch"
  - id: P3
    story: "a holder of a time right runs within its budget every period, everything else shares the rest fairly by weight, and every thread's run time is metered"
    state: proving
    gate: "the host model green on the kernel's own scheduler core; KSELFTEST: sched time right ok (the budget honoured over 100 periods, the overrun counted), sched fair ok (two weights within 5 % of their ratio), sched runtime metered ok; board: wake-to-run p99 <= 200 us for a time-right holder (moved from P4)"
    started: 2026-11-17
  - id: P4
    story: "no periodic tick: each hart's timer is armed for its next deadline and an idle hart sleeps until an event"
    state: done
    gate: "board: timer interrupts on an idle hart <= 2 per second (100 today); wake-to-run p99 <= 200 us for a time-right holder"
    started: 2026-11-16
    done: 2026-11-17
    evidence: build/logs/board--2026-11-17T11-06-45
    note: "Committed 9ad729e0. Gate's idle clause met: an idle system's harts wake only for the 500 ms net, 20 per hart in 10 s = 2/s (100/s before), board--2026-11-17T11-06-45. The empty wakes under use (17-29 per busiest hart, all stale) go to P8 (per-hart deadline sets); the time-right clause (wake-to-run p99 <= 200 us) moved to P3, where time rights exist."
  - id: P5
    story: "a woken thread runs where it runs soonest: placement by load, by its last hart and by cluster, and an idle hart pulls work"
    state: done
    gate: "QEMU -smp 8: 8 runnable compute threads occupy 8 harts within 2 ms (KSELFTEST: sched spread ok); board: the per-hart utilisation line; test_reject_steal_against_affinity kept"
    started: 2026-11-17
    done: 2026-11-17
    evidence: build/logs/board--2026-11-17T14-48-03
    note: "BUILT 2026-11-17: sched/placement.rs (pure place(): home when it takes the task at once — free, the waker's own, or running a lower class — and no same-or-higher backlog; else a free hart in affinity, home's cluster first; host: 4 tests incl. test_reject_place_outside_affinity, eight_wakes_occupy_eight_harts), task::wake places + migrates (set_home_cpu), smp::free_harts (idle minus pending wake hint), kick_idle_thief deleted; KSELFTEST: sched spread ok (the rule on the machine's topology — not timed threads: the 2 ms clause is read on the board from placed= and the busy line); KINIT: hart busy pm (busy share per hart, placed wakes). QEMU smp8 green but the known MTTCG bkl flip; smp1 green; just check green. Image dev-f49c8ba1."
  - id: P0
    story: "what the scheduler is rebuilt from is known: an interactive wake's wait for a hart is attributed (behind a peer of its class on the hart it is pinned to, behind a lower class it did not preempt, or a hart asleep), and the task that yields 70 000 times a second is named"
    state: done
    gate: "board: KINIT: ready latency class=interactive carries the three causes as counts (their sum = the dispatches above 1 ms; were 2 967 of 5 380, mean 1 513 us, build/logs/board--2026-11-11T09-05-45) and KINIT: yield top names the three tasks with the most yields; a missed preemption counted above 0 is fixed before P2; the yield loop is replaced by a wait or stated as the lane's own"
    started: 2026-11-11
    done: 2026-11-13
    evidence: build/logs/board--2026-11-13T10-38-20
    note: "gate met on the board (closes in the ledger when P1 starts, rule 8): ten windows with the desktop in use, 273 295 interactive dispatches, 519 above 1 ms = 508 behind a peer on the pinned hart, 9 a hart asleep, 2 a lower class not preempted (examined before P2); yield top total=0 in use — the network owner's loop removed at its cause, the harness's ladder is the lane's own yielder; ADR-0098"
  - id: P6
    story: "a wait that carries a deadline ends at its deadline on an idle system: a waitset wait and a fence wait are woken by the timer like a receive and a send, by one deadline walk instead of three copies (two of which know only IPC), and the idle hart does not walk every task under the kernel lock on each turn"
    state: done
    gate: "host: one deadline walk covers receive, send, waitset and fence (test per kind); KSELFTEST: wait deadline ok — a waitset wait and a fence wait with a 5 ms deadline return within 10 ms while no other task makes a syscall; board: bkl takes idle= per 10 s window below 100 000 (were 911 709, build/logs/board--2026-11-11T11-05-00)"
    started: 2026-11-16
    done: 2026-11-16
    evidence: build/logs/board--2026-11-16T15-13-23
    note: "found 2026-11-11 while reading cycle 3: wake_expired_blocked runs only inside another task's scheduling syscall; wake_expired_ipc_deadlines (timer interrupt, cpu0 idle) wakes IpcRecv/IpcSend only. Not the cause of that cycle's freezes (TASK-0330 P6). Part of P4's ground: a tickless hart needs the one walk. MEASURED 2026-11-16 (KINIT: bkl idle split, build/logs/board--2026-11-16T12-40-08): the boot hart's idle delivery holds the kernel lock 3.4-4.7 s of every 10 s — expired timers, interrupts and pressure 1.6-2.2 s, the walk over every task for an IPC deadline 1.4-2.0 s, 415 000-563 000 takes; after S3 it is the whole idle group (dispatch 0, steal under 0.15 s) BUILDING 2026-11-16: one walk — syscall::api::expire_deadlines (receive, send, waitset, fence; gated by budgets::NEXT_DEADLINE_NS, which every deadline block lowers and every walk sets exactly) serves the scheduling syscalls, the timer interrupt (was wake_expired_ipc_deadlines, IPC only — deleted) and the idle hart (was its own IPC-only walk on every turn — deleted); the re-arm after timer caps reads the gate (was earliest_blocked_deadline, a fourth walk over every task) — the walk runs first so the gate is exact. Next: the idle hart takes the kernel lock only when something is due (P4 with it) Idle gate built: the boot hart takes the kernel lock for its delivery only when something is due, read without the lock (cpu_main::idle_delivery_due: the deadline gate, timer::next_cap_ns — a new lower bound lowered by every arm, set by every expiry pass —, a stashed line, memory pressure, a 10 ms heartbeat for the liveness witness and a steal look, a stall); a secondary hart walks the deadlines itself when the gate is due (its own timer woke it for a task that blocked there — the boot hart may sleep). QEMU and board pending QEMU 2026-11-16 first run: idle takes per window 211 088 -> 942 (smp1), the walk 0.1 ms a window; two reds read: (1) usb-visible lost one of 28 fast keys — a timer cap armed on another hart was delivered only by the boot hart, asleep: every idle hart now delivers a due timer cap as it walks a due deadline (cpu_main::timed_due), the cap table is global under the lock; (2) smp8 bkl budget FAIL 22 ms — the secondaries took the lock on every wake to find their steal gate closed: the gate is decided before the lock now (steal_allowed), a hart takes it only for a steal look, a stashed line or a due deadline/cap\nsplit from P6 on 2026-11-16: P6's board clause is met (board--2026-11-16T15-13-23: idle takes 15 798 a window, under 100 000; were ~540 000), its host and selftest clauses are this phase. The walk is syscall::api::expire_deadlines (target-gated today): its per-task decision moves into a pure, host-tested function BOARD build/logs/board--2026-11-16T15-13-23: gate met — idle takes 15 798 a window (under 100 000; ~540 000 before), idle lock time 91 ms (3 974 ms), the interactive wake as before (le100us 98.37 %, late_idle 2), ipc call budget 28 us, bkl budget 505 us. Host and selftest clauses split into P7. Closes with the commit (rule 8)"
  - id: P7
    story: "a timed wait ends at its deadline on an idle system, proven per kind: a waitset wait and a fence wait with a short deadline return in time while no other task makes a syscall, and the one deadline walk is tested for receive, send, waitset and fence"
    state: done
    gate: "host: the walk's decision (task/deadline_rule.rs) with a test per case — expired at its deadline, pending before it, untimed (0, or a kind without a clock), the gate = the earliest pending deadline; each kind's deadline read by one exhaustive match (BlockReason::deadline_ns); SELFTEST: wait deadline ok — a waitset with no member and a fence nobody signals, each with a 5 ms deadline, return TimedOut within 10 ms (QEMU lanes and board)"
    started: 2026-11-16
    done: 2026-11-16
    evidence: build/logs/board--2026-11-16T17-31-43
    note: "split from P6 on 2026-11-16: P6's board clause is met (board--2026-11-16T15-13-23: idle takes 15 798 a window, under 100 000; were ~540 000), its host and selftest clauses are this phase. The walk is syscall::api::expire_deadlines (target-gated today): its per-task decision moves into a pure, host-tested function BUILT 2026-11-16: task/deadline_rule.rs (3 host tests) decides the walk; BlockReason::deadline_ns one exhaustive match; SELFTEST: wait deadline ok uses a waitset over the harness's kernel timer (no raw waitset deadline: check-wait-not-poll rule 4) and a fence deadline. The marker found a kernel bug: the preemption tick (timer_arm, on every dispatch and the S-mode re-arm) wrote the compare register past a pending earlier deadline while the ADR-0052 shadow still read armed — every kernel deadline up to 10 ms late (fence 10.4 ms, timer cap 5.6-12.5 ms for 5 ms). Fixed: the tick keeps a pending earlier deadline. QEMU 1/8 harts + usb-visible: 5.1-6.3 ms. BOARD build/logs/board--2026-11-16T17-31-43: waitset 5 025 us, fence 5 033 us; wake le100us 98.38 %, 1-10 ms 0.048 % (0.070), gt10ms 11.8 per 100k (8-12 before), timer takes 44 -> 808 a window (deadlines now fire), ipc call 44 us; frames 66/23, inputs 133/10, launches 6/0. Closes with the commit (rule 8)"
  - id: P8
    story: "each hart owns the deadlines of the tasks blocked on it: a deadline already served or no longer waited for never wakes a hart, and no hart arms another's deadline as a backstop"
    state: done
    gate: "board: KINIT: idle timer wakes max_empty <= 20 per 10 s window under use (17-29 at P4, all stale, build/logs/board--2026-11-17T11-06-45); stale= near 0; SELFTEST: wait deadline ok kept; late_idle=0 kept"
    started: 2026-11-17
    done: 2026-11-17
    evidence: build/logs/board--2026-11-17T12-09-56
    note: "Committed 221b7682."
---

<!-- `nx ledger new task` copies everything below this line; the frontmatter above shows the
     planning fields (RFC-0115). Write state with `nx ledger`, never by hand. -->

## The ask

The operator, 2026-11-10: finish SMP first — every hart, the kernel lock deleted — as the end
form, piece by piece, nothing rebuilt later; and take `TRACK-TIME-AS-RESOURCE` into the lane,
because a scheduler built on class labels would be rebuilt for it. The outcome: a thread that
holds a time right runs within its budget every period on a loaded 8-hart board, and no
run-queue operation waits for the kernel lock.

## Measured before the cut

- `build/logs/board--2026-11-13T10-38-20` (the desktop in use, ten windows): 273 295 interactive
  dispatches, 519 later than 1 ms — 508 behind a peer of the same class on the hart both are
  pinned to, 9 behind a hart asleep, 2 behind a lower class. A class cannot order its own
  members; that is what a deadline is for.
- Every run queue lives inside the kernel lock (`core/trap/runtime.rs`); strict classes, a 10 ms
  tick, steal one task per millisecond (`sched/mod.rs`); `sched/tests.rs` runs nowhere.
- `build/logs/board--2026-11-11T11-05-00`: 911 709 takes of the kernel lock by idle harts per
  10 s window (the deadline walk under the lock, P6).

## Three lenses

- **Scope** — in: the time right's contract and its pure core (`nexus-sched::time_right`, P1),
  run queues under a lock per hart (P2), the fair class and reservations with metering (P3), no
  periodic tick (P4), placement (P5), one deadline walk (P6) · out: the syscall surface and the
  capability kind (seeded in P3, appended to RFC-0120), a call lending the caller's reservation
  (TASK-0330, the IPC package), who is given which share (TASK-0365 P1), the supervision path's
  reservation and exhaustion as an event (after the lock is deleted, the track's candidates
  010 and 030).
- **Invariant** — every share of the root is held exactly once, no hart is reserved past its
  capacity, nothing of a revoked right runs, a reservation runs at most `share × time + one
  budget` through any steal or migration: `test_reject_delegate_beyond_held`,
  `test_reject_admit_beyond_capacity`, `test_reject_run_after_revoke`,
  `test_reject_migrate_beyond_the_targets_capacity`, `conservation_over_10_000_random_steps`
  (`source/libs/nexus-sched/tests/time_right.rs`).
- **Contract** — RFC-0120 (seeded here; answers the track's gate G3, states G1 and G2);
  ADR-0052 (per-hart earliest deadline, steal by affinity) stays; ADR-0098 gives the order the
  kernel lock is split in.

## Not doing here

- No guarantee marker before the kernel lock is deleted (gate G1): until then the mechanism's
  lines are measurements — the address-space package of TASK-0330 closes it.
- No slice table, no global deadline queue (RFC-0120, alternatives).
- No frequency or thermal control (the seam is the per-hart utilisation line of P5; the
  regulation is its own task).
- The exact release time of a hart a reservation left (its zero-lag point instead of its
  deadline): measured in P5 only if moves prove frequent.

## Definition of Done

- Every phase `done` with its evidence; the goal's target reached on the instrument.
- Host tests, the QEMU lanes and the board cycle the phases name are green.
- Security-relevant work has its `test_reject_*` and hardening markers
  (`docs/standards/SECURITY_STANDARDS.md`).
- Docs swept: CHANGELOG, affected docs, the RFC's status; `active_days` and, when the estimate
  missed, a `variance_note`.
