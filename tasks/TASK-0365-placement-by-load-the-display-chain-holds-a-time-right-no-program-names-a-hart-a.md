---
schema: 1
id: TASK-0365
title: "Placement by load: the display chain holds a time right, no program names a hart, and an exhausted budget is an event"
status: in_progress
owner: ["@kernel-team", "@runtime"]
created: 2026-11-10
updated: 2026-11-18
subsystem: kernel
kind: service
novelty: variant
estimate: 3
started: 2026-11-18
dates_source: recorded
goal:
  metric: "harts the display chain runs on within one frame; literal CPU masks in userspace; the compositor's wait from owing a frame to sending it"
  baseline: "the chain (gpud, windowd, inputd, hidrawd, xhcid, touchd, imed) is pinned to hart 0; literal masks in init affinity.rs, execd sched_recipe.rs, nexus-workpool and pinched; windowd hops frame owed>sent mean 11-27 ms, max 92-321 ms (board--2026-11-10T14-55-42)"
  target: "the chain on >= 3 harts within one frame; 0 literal masks (retired-name gate); owed>sent mean <= 2 ms on the board"
  instrument: "the scheduler's per-hart utilisation line; windowd: hops frame; the retired-name gate"
consumer: hardware-network/LU.1
phases:
  - id: P1
    story: "init derives the display chain's time right and hands it down: windowd, gpud, inputd and app-host hold a share, and app-host gives an app only what it holds"
    state: done
    gate: "init: time rights line (root, delegated, sum <= root); test_reject_an_app_claims_more_than_app_host_holds; the soft-realtime list is one table for the right and the class"
    started: 2026-11-18
    done: 2026-11-18
    evidence: build/logs/board--2026-11-18T09-41-28
  - id: P2
    story: "no program names a hart: service placement, the sched recipes and the worker counts of the workpool and pinched come from the exported topology and from roles"
    state: proving
    gate: "retired-name gate for the literal masks and MAX_WORKERS; the one-hart lane green (the same roles on one hart)"
    started: 2026-11-18
  - id: P3
    story: "a frame's threads run side by side: the chain on several harts of one cluster, background work on the other"
    state: planned
    gate: "board: >= 3 harts busy with the chain in a frame window (utilisation line); windowd: hops frame owed>sent mean <= 2 ms; gpud: ux frame ok in the idle-desktop windows"
  - id: P4
    story: "the supervision path holds a time right of its own and an exhausted budget is an event its supervisor receives"
    state: planned
    gate: "SELFTEST: supervision right ok (a spinning service delays a restart by at most one period); SELFTEST: budget exhausted event ok; TRACK-TIME-AS-RESOURCE candidates 010-030 closed"
---

<!-- `nx ledger new task` copies everything below this line; the frontmatter above shows the
     planning fields (RFC-0115). Write state with `nx ledger`, never by hand. -->

## The ask

The operator's plan for the SMP lane (2026-11-10): the display chain holds a time right, the
chain runs on several harts, no program names a hart — the consumer of TASK-0363's mechanism.

## Measured before the cut

- `build/logs/board--2026-11-18T08-25-01` (TASK-0363 P3): init binds three reservations from
  one right it alone holds (`init: time rights … bound=0x3`); no service and no app holds a
  capability. 99.26 % of 163 507 holder wakes within 200 us; cpu0 is the busiest hart in every
  window (the chain's fixed role).
- `board--2026-11-10T14-55-42`: windowd `owed>sent` mean 11–27 ms, max 92–321 ms (P3's number).

## Three lenses (P1)

- **Scope** — in: a time right travels parent → child as a capability; a grain per right; init's
  one table for class and time; a right per chain service and the apps' right; execd hands each
  app a part and revokes it at the reap. · out: binding an app's right (TASK-0367 P2, the
  painter's bands, and P3 here — a holder must migrate by load first); periods from the
  display's refresh (P3, with a measurement); the masks (P2).
- **Invariant** — a holder takes from a right only what it holds, in parts no finer than its
  grain, and a right goes only into a direct child:
  `KSELFTEST: test_reject_an_app_claims_more_than_app_host_holds`,
  `test_reject_a_part_finer_than_the_rights_grain`,
  `test_reject_manage_of_an_endpoint_in_a_transfer`.
- **Contract** — RFC-0120 (the grain, the capability handed down), RFC-0112 (transfer targets).

## Three lenses (P2)

Measured before: init and execd already place by role (RFC-0118). Left naming a count or a CPU:
`nexus-workpool` (`MAX_WORKERS = 4`, worker `i` pins itself to the `i`-th online CPU, four
static stacks) and pinched (`PINCHED_WORKERS = 2`).

- **Scope** — in: the pool sized from the CPUs its process may run on, no self-pin, stacks from
  one lazy mapping; pinched and the selftest name no number; a restarted service placed like at
  boot (found by the new line). · out: waking through a word, the caller's time right, the
  scaling number (TASK-0366 P3); queue, cancellation, isolation (TASK-0368).
- **Invariant** — the worker count is `|own set ∩ online|`, at least one, on any machine
  (`a_pool_has_a_worker_per_cpu_its_process_may_run_on`, `SELFTEST: workpool sized ok`); the
  result is the same for every count (`every_worker_count_a_machine_can_give_produces_identical_results`);
  every pinched instance of a run names the same count (`scripts/qemu-test.sh`).
- **Contract** — RFC-0118 (roles against the online set), ADR-0046 (the pool's determinism).

## Not doing here

- A call lending the caller's right to its server: TASK-0330 P2.
- Threads of a process sharing its right: TASK-0364.

## Definition of Done

- Every phase `done` with its evidence; the goal's target reached on the instrument.
- Host tests, the QEMU lanes and the board cycle the phases name are green.
- Security-relevant work has its `test_reject_*` and hardening markers
  (`docs/standards/SECURITY_STANDARDS.md`).
- Docs swept: CHANGELOG, affected docs, the RFC's status; `active_days` and, when the estimate
  missed, a `variance_note`.
