<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0102: A thread blocks in two steps and a waker never waits — IPC leaves the kernel lock on state at fixed addresses

- Status: Proposed (2026-11-24, TASK-0330 P2)
- Date: 2026-11-24
- Links:
  - Tasks: `tasks/TASK-0330-smp-v3-eight-harts-fdt-topology-bkl-decomposition-per-asid-shootdown.md` (P2)
  - RFCs: `docs/rfcs/RFC-0102-smp-lock-decomposition-lock-classes-ranks-and-budgets.md` (the classes and ranks; amended here in one point: how an object is found)
  - Related ADRs: `docs/adr/0098-the-kernel-lock-is-split-in-the-order-the-desktop-in-use-holds-it.md` (the order), `docs/adr/0100-a-queued-task-is-the-hart-s-that-takes-it-the-dispatch-takes-no-kernel-lock.md` (the record and its owner), `docs/adr/0073-an-object-dies-with-its-last-capability-a-death-is-an-event-on-an-endpoint.md` (an object's life)

## Context

With the desktop loaded on eight harts (`board--2026-11-23T18-35-25`) the IPC syscalls take the
kernel lock 46 000–121 000 times in 10 s and hold it 0.25–0.66 s: every send, receive and
waitset wait of one pair of services queues behind every other pair, and behind `vmo_runs`
(1.3–1.9 ms a call; one hart waited 10.7 ms). RFC-0102 names the lock classes and their ranks.
It does not say how a thread goes to sleep without the one lock that today makes "check the
queue, register, mark blocked, leave the run queue" one step, where the state lives that a waker
on another hart reads, how an object is found, or how a syscall copies to user memory while
another hart may unmap it. An IPC syscall today reads all of that from tables that move when
they grow (`Vec<Task>`, `Vec<Endpoint>`) — the kernel lock's.

Two properties of this kernel carry the decision. A syscall runs to completion on its hart's one
kernel stack, so no lock is ever held across a switch. And a syscall that blocks **re-executes**
when its thread runs again: the frame saved at the trap's entry is the thread's whole state, so
a wake is a hint and a wake too many costs one retry.

## Decision

- **No flag day.** An object first gets its own lock while its callers still hold the kernel
  lock; a syscall leaves the kernel lock when everything it touches is the object's. Until P4
  the kernel lock ranks above rank 1: it may be held while a ranked lock is taken, never taken
  under one.
- **State at fixed addresses.** Endpoints live in a block table like the scheduling records
  (blocks of 64, never freed): an id is found without a lock. A dead slot is reused in place —
  once no capability names it **and nobody holds its header**. A syscall that came through a
  capability takes the endpoint's header from the slot of its table, under the table's lock, and
  keeps it for its length (`cap::PinnedEndpoint`); a caller waiting in `ipc_call` keeps its
  reply endpoint's the same way, since the reply capability travelled with the request. So an
  id a syscall works on is the endpoint its capability named, whatever a parent does to the
  slot meanwhile. The registry lock of RFC-0102 guards creation and reuse, not the lookup: a
  shared line touched by every message would be the next global lock. A thread's wait word and
  what a wake reads under the thread lock (class, frozen, the committed call) live in its
  record, its home hart and affinity as words beside that lock (a thief reads them under a
  queue lock, rank 7); a task's capability table sits in its record behind its own lock
  (rank 2). Waitsets and fences live in a fixed table of slots, each with its lock.
- **A thread blocks in two steps**, on a wait word under the thread lock (rank 6):
  `Running → Preparing → Blocked`, with `Woken` beside it.
  1. *Prepare:* the thread becomes `Preparing`, then registers on each object under that
     object's lock and re-checks the condition there.
  2. *Commit:* in one hold of the thread lock, `Preparing` becomes `Blocked`, the thread leaves
     the run queues and its record is released. The hart touches the thread no more.
  - *Wake:* in one hold, `Blocked` becomes runnable — placed, enqueued (rank 7) — and the hart
    is told after the hold; `Preparing` becomes `Woken`; anything else is left alone. A commit
    that finds `Woken` returns to `Running` and the syscall checks again.
  - No wake is lost: registration precedes the commit, and a waker takes the registration off
    the object before it wakes. No thread runs twice: the record is released in the hold that
    makes it wakeable. A waker waits for no hart. A deadline is part of `Blocked`; the hart
    that owns it wakes through the same step.
- **User memory is copied without a lock**, on the guarantee the unmap already gives: it clears,
  waits for every non-idle hart's acknowledgement, then frees. A hart in a trap acknowledges
  only where it polls — a lock's spin, its own shootdown. So: between a translation and the
  last byte copied through it, a hart takes no lock and orders no shootdown; every build marks
  the stretch (`smp::tlb::NoAck`) and a poll inside one panics. A copy runs outside every object
  lock — a message is taken under the endpoint's lock and copied after. A page that is not
  mapped and usable is a miss, never a fault: the copy fills no lazy page.
- **One body, two entries.** Each IPC syscall is one body over the caller's record, the router
  and the two tables. The trap runs it before the kernel lock; the syscall table runs the same
  body under it, after committing the caller's pages. A body asks for the lock when a copy
  misses or an order waits for its thread — it has changed nothing by then, or put it back (a
  taken message returns to the head of its queue, a moved capability to its slot) — and the
  syscall re-executes whole under the lock. A thread that blocked leaves its hart through the
  hart's scheduler loop: the loop's pick is the switch, the trap picks nothing. A task this
  syscall woke that goes ahead of the caller (ADR-0096) is reached the same way: the caller's
  frame is completed, it is requeued, the loop picks.
- **An order for another thread** (a kill, a freeze) is still given under the kernel lock, but a
  wake no longer takes that lock. So the order is decided under the target's thread lock: a
  thread found off every queue and running nowhere is marked parked in that hold — a wake from
  then on ends its block and enqueues nothing — and one that runs gets the order as a word in
  its record and is woken; going to sleep, it reads the word after its prepare.
- **A call lends the caller's time right** (`sched/loan.rs`). A caller that runs with a
  reservation (RFC-0120) lends it to the thread its call wakes, if that thread holds none and
  may run on the reservation's hart: the word is written before the wake, so the server is
  placed on that hart and arrives with the reservation's deadline and what is left of its
  budget, and what it runs is charged there. Nothing is admitted and nothing is added — one
  reservation, one thread running with it at a time. It goes back when the lender is woken
  (the server's reply; any other wake), when the lender's call returns without a wake, when
  the server waits for anything but the answer to a call of its own, when either exits, and
  when the lender's reservation is rebound or unbound. A server that calls on lends what it
  runs with, and taking back follows that chain. The loan is three words in the two scheduling
  records, settled with one compare-and-swap and no lock; a loan that loses a race is not made,
  and the server runs as it did before. A server that was not waiting when the request arrived
  borrows nothing: the message carries no right, only the wake does.
- **Counters, not maps.** The router's queued-byte total is an atomic (reserved, given back on a
  refusal), an owner's total one counter that the owner's endpoints share. Capabilities in messages a close
  drops are dropped by the closer after it released the endpoint, from its own list.
- **A hart gives a thread up and its address space with it** (2026-11-25). Under the kernel
  lock the trap's exit moves the hart to its next space and a reap waits for that lock. Off it,
  a hart that stayed on the space of a thread it no longer runs is found there by the reap of
  that thread on another hart, which then keeps the space (`address space free FAIL`, QEMU
  eight harts, 2 of 4 runs once the steal spread a workpool's threads within microseconds).
  So the hart enters the kernel's own space in the hold that makes the thread another hart's:
  its thread lock at the commit of a block — before a waker or a kill's `park` can find it
  blocked — and its queue's lock at a requeue (`hart_spaces::enter_kernel_space`). The
  kernel's space is named once at boot as the machine's; read from PID 0's record it was lost
  to a selftest's attach, and the leave did nothing for a whole boot. Rejected:
  leaving the hart on the space and deferring the free to the hart's next switch — no switch
  on a block-and-wake on one hart, but a dead process's memory then waits for an idle hart.
- **A deadline's bound is two words** (2026-11-25). A thread that blocks with a deadline lowers
  the walk's gate and its hart's floor; the walk over all waiters set both exact by storing its
  result. Under one lock that was sound. Off it, a thread that blocked after the walk looked at
  it and before the walk stored was in no bound: its timeout was delivered by the next walk for
  another thread's. Now waiters lower `noted`, a walk clears it as it begins and stores into
  `walked`, and a reader takes the minimum of both under the walks' sequence, again if a walk
  ran beside it (`sync::noted_bound`, every interleaving of a waiter, two walks and a reader
  on the host). The block lowers inside the hold of the thread lock that sets its reason, and
  the walk counts a wait that is still preparing as pending. Rejected: a reader that takes
  "due now" beside a walk — a hart about to sleep stored it as its armed deadline, and lost the
  one it had armed until its next tick (seen once: a 5 ms timeout after 10.7 ms).
- **An endpoint dies off the kernel lock** (2026-11-25, TASK-0330 P4). `cap_close` is one
  body for every kind on either path. The closer keeps the dropped capability's header
  (`cap::Dropped`) until its judgement — EOF, `PeerClosed`, the death, the forget — is done, so
  the id stays that endpoint's. Waitsets, timers, lines and the pressure sink forget it under
  their own locks; a child-exit binding holds the endpoint instead (`PinnedEndpoint`), so no
  death walks the tasks. A locked path that stores an endpoint's id pins it and reads its life
  after the store (`ipc_reclaim::stored`): the closer marks it dead before it forgets, so one
  of the two finds the store. A line and the sink serialize bind and forget on their own
  lock. Rejected: a generation in the endpoint id (a new ABI; the pin already exists).
- **What leaves the lock in P2:** send, receive, call and reply, waitset wait, fence wait and
  signal. Teardown (a process's exit, the expiry walk) stays under the kernel lock until P4 and
  takes the objects' locks like everyone.

## Consequences

- A blocking syscall is written prepare → register → commit; a path that marks a thread blocked
  any other way is a bug the wait word's own transitions refuse (host-tested, with
  `test_reject_*` for a commit without a prepare and a wake of a running thread).
- The stress lane (`just ci-os-smp8`) judges it: the whole service ladder on eight harts with
  the kernel built to check the lock order — a lock taken against its rank, a ranked lock held
  past an IPC syscall, a poll for a shootdown inside a copy and a frame saved twice
  (`frame owner`) are panics, and a receiver left blocked on a queue that holds a message is a
  rung of the ladder that never comes.
- A wake costs one more load (the lender word), a block one more (the borrower word); a call
  by a task with no reservation costs two loads and lends nothing.
- Refuted if the board still names an IPC syscall among the kernel lock's holders, or
  `KSELFTEST: ipc call budget` misses 64 us on any hart: then a step is wrong and is reverted.

## Alternatives considered

- **Mark blocked under the endpoint's lock** (endpoint, then thread, in one nest). Enough for
  one endpoint, wrong for a waitset: a message arriving at the first member while the second is
  registered finds the thread not yet blocked.
- **A lock per address space around every user copy.** P3 brings that lock for the page tables'
  writers; a reader needs none while frees wait for acknowledgements.
- **A registry lock and a counted reference per lookup** (RFC-0102 as written). One shared
  cache line per message on eight harts.
- **Wait for the thread's hart in the waker.** A waker inside an endpoint's hold would spin on
  another hart's syscall.
