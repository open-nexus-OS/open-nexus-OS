<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0073: Every kernel object dies with its last capability, and a process's death is an event the kernel writes into an endpoint its parent bound

- Status: Accepted
- Date: 2026-10-20
- Links:
  - Tasks: `tasks/TASK-0333-memory-the-efficient-one-and-the-app-lifecycle-layer-6.md` (6a)
  - RFCs: `docs/rfcs/RFC-0100-memory-object-model-v2-commit-on-touch-unified-gpu-memory-pressure.md` (§1),
    `docs/rfcs/RFC-0079-…` (last-sender EOF), `docs/rfcs/RFC-0081-process-reaper-nonblocking-reclaim.md`
  - Related ADRs: `docs/adr/0056-…` (exit reasons are kernel truth), `docs/adr/0062-…` (stage fences)
  - Measurement: `docs/board/measurements/2026-10-20-memory-lifecycle-baseline/README.md`

## Context

Layer 6a asked what the system gives back when a task exits. The board answered: an app closed
with its window's close button never exited. Its app-host waited on a waitset and received
non-blocking with EOF opted in; the kernel decided EOF only in the *blocking* branch of `recv`,
so the receiver saw `WouldBlock`, consumed the latch, and parked forever on a peer already gone
(cycle 45: nine launches, nine live processes, abilitymgr counting instances up to its table's
end). Behind it, three more facts of the same kind: an endpoint lived until its *owner* exited
(the two pairs init mints per app launch were owned by execd and closed by nobody — 384 ids,
never reused, 96 per owner: a launch counter), an exit swapped the capability table without
freeing the timers, waitsets and fences the task owned, and a sender had no way to learn that
no receiver could ever drain its queue (windowd parks on a crashed app). The reaper of record
(RFC-0081) swept only on its next request, so a dead app's address space stayed until somebody
launched something.

The lifecycle layer needs one rule for all of this, and it needs death to be reactive: a phone's
window server and lifecycle authority learn of a process's death from the kernel, at once, and
never by asking.

## Decision

- **Every kernel object dies with its last capability.** RFC-0100 §1's rule for VMOs applies to
  endpoints, timers, waitsets and fences: an object nothing names — no capability in any
  table, none moving inside a queued message, no mapping for a VMO — is destroyed, and
  everything that remembered its id forgets it (waitset member lists, timers notifying it,
  interrupt lines bound to it, parents that bound it for their children's deaths). A dead
  endpoint's slot is reused only once nothing names its id; a stale capability keeps naming
  nothing. The owner's exit still closes what it owned (RFC-0005) — on every exit path, a
  fault kill included.
- **An exit is one funnel that releases like a `cap_close` of each capability** — the image,
  the owned endpoints, the owned timers/waitsets/fences, then the table — and judges every
  endpoint the table named: receivers whose last peer left wake to `PeerClosed` (RFC-0079),
  senders whose last receiver left likewise, orphans are destroyed.
- **The EOF verdict never depends on the wait mode.** A non-blocking, EOF-opted receive on an
  empty queue returns `PeerClosed` exactly when a blocking one would.
- **A send into an endpoint no RECV capability names returns `PeerClosed`** — the sender's
  mirror of last-sender EOF; a sender parked on such a queue is woken to it. The owner counts
  as a receiver here: a receive needs the capability, whoever holds it.
- **A process's death is an event on an endpoint.** `SYSCALL_CHILD_EXIT_BIND` (62) binds a
  SEND capability of the caller's endpoint as the sink for its children's deaths; the exit
  funnel writes one `OP_CHILD_EXITED` frame (pid, code, the kernel's reason word of ADR-0056)
  into it, the shape of a timer's fire and an interrupt's. The frame is a wake, the zombie
  table is the truth: the parent sweeps with `wait_nohang`, so a dropped frame under a burst
  delays nothing past the next death. No policy enters the kernel: it knows parents and
  children, never apps.
- **Ownership of the report.** execd — the reaper of record — waits on its declared
  child-death endpoint beside its server endpoint and reaps on the wake; an app child's death
  goes to abilitymgr as `OP_INSTANCE_EXITED`, a one-way event abilitymgr takes from execd's
  kernel sender id alone. abilitymgr moves the instance to `Stopped` and removes it: a dead
  process holds no instance slot. windowd learns of a dead client by the kernel's name
  (`PeerClosed`/`NoSuchEndpoint` on its next push) and closes the window as the close button
  would. Out of scope here: a live-but-slow client (windowd's delivery contract, 6d), freeze and
  kill orders (6d), restore (6e).

## Consequences

- **Positive**: closing a window ends the process and returns everything it held, on the kernel's
  initiative; the launch counters are gone (endpoint ids reused, timers/waitsets/fences freed,
  instance slots freed); every death reaches the reaper at once; no service parks on a dead
  peer; one rule covers VMOs and IPC objects alike, host-tested as pure predicates.
- **Negative / accepted cost**: one more syscall (62) and one more declared pair in execd's slot
  table; `MmStats` grows four fields (version 2); a capability close now runs two scans over the
  task tables (bounded, human-rate paths: closes and exits).
- **Follow-ups**: 6d (windowd never blocks on an app; freeze/kill orders through execd; the
  ghost band), 6e (`memd`, restore before the first frame).

## Alternatives considered

- A per-child "lifetime" channel whose EOF the parent waits on (one endpoint per child): the same
  signal with no new syscall, but a mint per spawn, a capability slot in every child and a
  waitset member per child — the one-endpoint, keyed-frame shape matches the kernel's timers
  and interrupts and a phone's process-exit notifications.
- Reaping on the next request (RFC-0081 as it was): correct but late — a dead app's address
  space and the lifecycle authority's instance stayed until the next launch.
- Reference counts kept in the router per endpoint: cheaper than a scan, but a second truth
  beside the capability tables that every clone, transfer, close and exit would have to keep in
  step; the scan reads the one truth and runs only on closes and exits.
- Letting the app own its channels so they die with it (the pure handle-dies-with-process
  shape): needs init to mint for a grandchild, which the factory rule forbids by design; the
  reclaim by last capability reaches the same end without widening that authority.
