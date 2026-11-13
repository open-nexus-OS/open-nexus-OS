---
schema: 1
id: RFC-0120
title: "Time is a right: a share of the harts' time derived as a subset, a reservation admitted per hart, revoked in one step"
status: draft
owners: ["@kernel-team"]
created: 2026-11-13
updated: 2026-11-13
subsystem: kernel
tasks: [TASK-0363]
depends_on: [RFC-0118]
links:
  - label: "The track this answers (gates G1–G3)"
    target: tasks/TRACK-TIME-AS-RESOURCE.md
  - label: "Per-hart earliest deadline, steal by affinity"
    target: docs/adr/0052-per-hart-earliest-deadline-timer-and-affinity-respecting-steal.md
  - label: "The kernel lock's split order"
    target: docs/adr/0098-the-kernel-lock-is-split-in-the-order-the-desktop-in-use-holds-it.md
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# RFC-0120: Time is a right

## Why

A QoS class is a label any service may claim; when everything is `interactive`, nothing is.
Measured on the board with the desktop in use (`build/logs/board--2026-11-13T10-38-20`, TASK-0363
P0): 273 295 interactive dispatches in ten windows, 519 of them later than 1 ms — 508 behind a
**peer of the same class** on the hart both are pinned to, 9 behind a hart asleep, 2 behind a
lower class. A class cannot order two members of itself; a deadline can, and a deadline is only
honest when the time it promises was given to its holder and to nobody else.

The scheduler is rebuilt once (run queues off the kernel lock, TASK-0363 P2–P5). Built on
labels, it would be rebuilt again for `TRACK-TIME-AS-RESOURCE`. This RFC decides the contract
that rebuild implements — on a host model first (TASK-0363 P1), so no kernel code is written
against an undecided contract.

## Scope

- **Owns:** what a time right and a reservation are, the operations on them and their refusals,
  what a steal and a migration do to a reservation, the bound on a revoke, the invariants and
  the model that proves them (`nexus_sched::time_right`).
- **Does not own:** the syscalls and the capability kind that carry these operations (seeded in
  P3 with the kernel core, appended to this RFC); the run queues and their locks (P2); the fair
  class's weights (P3); tickless timers (P4); placement (P5, TASK-0365); a call lending the
  caller's reservation to the server (the IPC package, TASK-0330); who is given which share
  (init's recipe, TASK-0365 P1).

## Contract (normative)

### Two objects

- A **time right** is a share of the harts' time, in parts per million of one hart (`PPM`), held
  in a derivation tree of depth ≤ 8. The root holds `harts × capacity`; `capacity` is the part
  of a hart reservations may fill (the rest is the fair class's floor — everything without a
  right still runs). A right records what it was given (`total`) and what of it is neither
  derived away nor bound (`own`).
- A **reservation** is the part of a right one thread runs with on one hart: a share ≤ one hart
  and a period in `[1 ms, 1 s]`, giving a budget of `share × period` (≥ 50 µs) each period, a
  deadline at the period's end, earliest deadline first on its hart. A right is a holding; only
  a reservation is scheduled.

### Operations and refusals

| Operation | Does | Refused with |
|---|---|---|
| `derive(parent, share)` | a child right of `share`, taken from the parent's `own` | `Revoked`, `Share` (< 1 000 ppm), `BeyondHeld`, `TooDeep`, `Full` |
| `revoke(right)` | the right and everything derived from it; its `total` is the parent's `own` again | `Revoked`, `Root` |
| `bind(right, share, period, hart, now)` | a reservation, taken from the right's `own`, admitted on `hart` | `Revoked`, `Share`, `Period`, `BeyondHeld`, `Hart`, `BeyondCapacity`, `Full` |
| `unbind(reservation, now)` | ends it; its share is the right's `own` again | `Revoked` |
| `migrate(reservation, to, now)` | moves it to another hart, admitted there | `Revoked`, `Hart`, `BeyondCapacity` |
| `charge(reservation, ns, now)` | takes run time from this period's budget, at most what is left | `Revoked`, `Exhausted` |
| `replenish(reservation, now)` | at or after the deadline: a full budget, the deadline moved on by whole periods | `Revoked`, `Early` |

A refusal changes nothing. An id carries the generation its slot had when it was made: an id of
an earlier generation never names a later right.

### Admission

A hart's reservations sum to at most `capacity`. Earliest deadline first on one hart meets
every deadline exactly when the shares sum to at most one hart; admission is that test, per
hart. A right may hold more than fits on any one hart (`BeyondCapacity` is a property of a
hart, `BeyondHeld` of a right): holding is conserved, running is admitted.

### What a steal and a migration do (the track's gate G3)

The track asked whether conservation survives work stealing and reallocation across periods
with a dynamic contingent instead of a slice table. It does, under three rules, each a test:

1. **A reservation moves whole.** Its budget of the period and its deadline belong to the
   reservation, not to a hart; a steal by an idle hart is a migration by another caller. No
   budget is created or lost by a move (`audit` after every step of the random walk).
2. **The target admits it** like a bind — a move that would over-commit the target is refused
   and leaves the reservation where it was (`test_reject_migrate_beyond_the_targets_capacity`).
   The **source stays reserved until the migrant's deadline**: what the migrant ran ahead of
   its share in this period is not handed out twice.
3. **A migrant never arrives denser than its share.** If what is left of its budget, over what
   is left of its period, exceeds its share, it starts a new period at the move (a full budget,
   a deadline one period away) — the constant-bandwidth rule. The target's residents never pay
   for a move; the migrant's deadline moves later instead
   (`a_migrant_never_arrives_denser_than_its_share`).

With these, over any interval a reservation runs at most `share × interval + one budget`
(asserted for every reservation after every step), so per-hart admission keeps its meaning
through any sequence of moves. A fair-class thread is stolen as today (ADR-0052: by affinity).

What the static table would have given and this waives: the isolation domains are not readable
from a table; they are a tree query (`held`) plus a per-hart sum (`load`). Reconfiguration is
not free; it is one bounded operation per change.

### Revocation is one step

`revoke` gives the share back to the parent and moves the right's generation on — at most
depth + 1 steps, independent of how much was derived from it. Nothing below it is walked: a
descendant right or a reservation is found dead when next touched (liveness is a walk up at most
eight parents), and the scheduler touches a reservation every time it picks, charges or
replenishes it. So a revoked right's thread stops being scheduled from its reservation at its
next scheduling decision — bounded by the budget it was running on — and its hart stays
reserved until that reservation's deadline (≤ 1 s, the period bound), after which the capacity
is free. The revoker's share is back at once; the hart's capacity within one period.

### The track's other gates

- **G2 (accounting):** `charge` is the meter; the kernel core calls it with the run time it
  measured per thread (P3, `KSELFTEST: sched runtime metered ok`).
- **G1 (bounded kernel operations):** every operation here is bounded by constants (depth 8,
  the tables' sizes). The *guarantee* a reservation states is void while kernel paths serialize
  behind the kernel lock: **no marker claims a guarantee before the lock is deleted** (the
  address-space package). Until then the mechanism runs and its lines are measurements.

## Invariants

| Invariant | Proof |
|---|---|
| Every share of the root is held exactly once: by a right itself or bound in a reservation of a standing right; each right's `own` + derived + bound = its `total` | `audit`; `conservation_over_10_000_random_steps` |
| A right is derived or bound only from what it still holds | `test_reject_delegate_beyond_held` |
| A hart is never reserved past its capacity — by a bind, a move, or a share handed out twice in a period | `test_reject_admit_beyond_capacity`, `test_reject_migrate_beyond_the_targets_capacity` |
| Nothing of a revoked right runs, is replenished, moved, bound or derived from | `test_reject_run_after_revoke` |
| A reservation runs at most its budget a period, and at most `share × time + one budget` through any moves | `a_reservation_runs_its_budget_once_a_period`; the random walk |
| Bounds: depth ≤ 8, share ≥ 1 000 ppm, period 1 ms–1 s, budget ≥ 50 µs | `test_reject_a_share_below_the_floor_and_a_tree_too_deep`, `test_reject_a_period_or_a_budget_out_of_bounds` |

## Failure model

- **Budget spent** (`Exhausted`): the thread leaves the deadline order until its deadline and
  runs in the fair class by its weight meanwhile; the exhaustion is counted and becomes an event
  to its supervisor (the track's candidate 030, after the lock is deleted).
- **No room on a hart** (`BeyondCapacity`): a bind is refused to its caller; a move is not made.
  Placement (P5) asks another hart; none having room is the caller's answer, never a silent
  over-commit.
- **Tables full** (`Full`): refused. The floor on a share bounds how many rights a holder can
  make from what it holds; the kernel's table sizes are decided in P3 against that bound.
- **Returning to a hart just left:** refused until the old deadline unless the hart has room for
  both — the conservative side of rule 2.

## Security

- A right is a capability: who may derive, bind or revoke is who holds it in its capability
  table (kernel identity, never a name or a payload). There is no operation that names a right
  by a string.
- No operation mints time: the root is fixed at boot from the hart count; `derive` and `bind`
  only take from `own`.
- A holder can starve only what it derived (it revokes its own subtree), never a sibling or its
  parent; a child cannot outlive its parent's revoke.
- Denial of service through the tables is bounded by the share floor and the depth; a share too
  small to pay for its switches is refused (`Period`).
- DON'T: accept a share, a period or a hart from userspace without these checks; treat a class
  label as a guarantee; print a guarantee marker before G1 holds.
- Open risk: a reservation's holder that never blocks consumes its whole budget every period —
  intended; the admission test is what protects the others.

## Proof

```bash
cargo test -p nexus-sched --test time_right
```

Kernel markers arrive with P3 (`KSELFTEST: sched time right ok`, `sched fair ok`, `sched runtime
metered ok`) and are added here with the syscall surface.

## Alternatives considered

- **Static slices `(hart, [from, to))`** — table lookup scheduling and isolation readable from
  state, at the price of a fixed allocation a desktop's frame loads cannot pay. Rejected by the
  track itself; the conservation principle is kept.
- **Global earliest deadline first** (one queue over all harts) — no per-hart admission, but one
  lock over every hart and no utilisation bound that is both simple and tight. Rejected: the
  package exists to take the scheduler off one lock.
- **Refill at exhaustion with a postponed deadline** (the soft variant) — work-conserving inside
  the class, but a holder that overruns then competes with honest holders at a later deadline.
  Rejected: an exhausted thread runs in the fair class, where its overrun costs only the floor.
- **Revoke by walking the subtree** — immediate everywhere, unbounded in the size of what was
  derived. Rejected: the recovery action must not relocate the outage (gate G1).
- **Releasing a left hart at once** — admits more, and hands out twice what a migrant ran ahead.
  Rejected; the exact release time (the migrant's zero-lag point, never later than its
  deadline) is a refinement to measure in P5 if moves prove frequent.

## Open questions

- The value of `capacity` (the model's tests use 80 %): decided in P3 from the fair class's
  measured need on the board.
- Table sizes in the kernel against the share floor and the depth (P3).
- A right bound by several threads on several harts is several reservations today; whether a
  thread group shares one budget (a frame's threads, the work interval of TASK-0365) is decided
  there, on this contract.
