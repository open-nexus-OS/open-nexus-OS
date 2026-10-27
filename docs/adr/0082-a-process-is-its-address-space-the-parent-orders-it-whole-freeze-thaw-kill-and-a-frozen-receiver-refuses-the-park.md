<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0082: A process is its address space — its parent orders it whole (freeze, thaw, kill), a leader's exit takes its threads, and a send that would park on a frozen receiver is refused

- Status: Accepted (2026-10-27; QEMU lanes + board cycle of TASK-0333 6d)
- Date: 2026-10-27
- Links:
  - Tasks: `tasks/TASK-0333-memory-the-efficient-one-and-the-app-lifecycle-layer-6.md` (6d-0, 6d-1)
  - RFCs: `docs/rfcs/RFC-0111-process-lifecycle-freeze-thaw-kill-the-parent-orders-the-process.md`
    (the contract this decision fixes the kernel half of), `docs/rfcs/RFC-0100-…` (§1 an object
    dies with its last capability — the reap this decision completes), `docs/rfcs/RFC-0079-…`
    (EOF: the never-park discipline this decision extends to frozen receivers)
  - Related ADRs: `docs/adr/0056-…` (exit reasons: `Killed` is the kernel's word), `docs/adr/0073-…`
    (death is an event: the parent's death endpoint carries an ordered kill like any exit),
    `docs/adr/0080-…` (a process runs from a sealed image: where `killable` is set)

## Context

Before 6d a task was the only unit the kernel knew. `task_resume` enqueued ANY pid (no parent
check); there was no suspend and no kill of another task; a task's exit released what it owned
(6a) but not what shared its address space. Measured on the board (cycles 46–71): the compute
broker's selftest crash left its two worker threads alive in the dead leader's space, so the
space stayed mapped (`mappers=3`) through every respawn — the "init reaps no dead child"
hypothesis of 6c-1b was wrong; the leak was orphaned threads. The lifecycle layer (6d-2) needs
to freeze a hidden app and kill one on the system's decision; without a kernel freeze an app
only "sleeps" when it chooses to, and without a process-wide order a kill of the leader leaves
threads running in a space that is being reclaimed.

The mobile systems this OS is measured against both decided this long ago: a process is the
unit of the lifecycle, frozen and killed whole (iOS suspends the whole process when hidden;
the freezer cgroup freezes every thread; a kill is of the process). And both learned the second
rule the hard way: a sender that blocks on a frozen receiver is a deadlock the system cannot
see — so the IPC layer refuses at once (`EDEADLK`-class errors in Binder's frozen-process
handling), and what fits the queue is delivered when the receiver thaws.

## Decision

- **A process is its address space.** The task that created the space is its leader; a task
  spawned INTO the parent's own space (`spawn` without a new space) is a thread of it
  (`is_thread`). Every order names a process and acts on leader and threads alike.
- **The parent orders the process; parentage is the authority.** `task_resume` (32),
  `task_suspend` (65) and `task_kill` (66) accept only a live child of the caller (`ESRCH`
  otherwise). `task_kill` additionally needs the child's `killable` bit, which its parent set
  once at `exec_vmo` (`EXEC_FLAG_KILLABLE`): the service launcher sets it for every child it
  spawns (apps, probes), init sets it for no boot service. `EPERM` otherwise. The kernel's own
  last resort (6e) honours the same bit.
- **A leader's exit takes its threads.** Every exit path runs the one funnel; a leader's funnel
  kills every live task of its space (`Killed`, code −9); a thread whose leader is gone has
  nobody to `wait` for it and is released by its own exit. The reap of the leader frees the
  space (`KSELFTEST: process exit takes threads ok (pid= threads= space=freed)`).
- **Frozen is `Suspended` + `frozen`.** A frozen task is never enqueued and never dispatched; a
  wake only clears its block. The thaw enqueues what is runnable and leaves the blocked to
  their wakes. The kernel proves the invariant once per boot at the first thaw of a frozen
  task (`KSELFTEST: task freeze ok (pid= resumed=1 frozen_ns= ran=0)`; a dispatch of a frozen
  task is `TASK: frozen task dispatched`).
- **An order for a task running on another hart is a pending mark plus a resched IPI.** That
  hart executes it at its next trap entry (syscall or interrupt), before anything else runs on
  the task's behalf — the way a signal is delivered on the return to user mode. A kill is never
  downgraded to a freeze. The frozen task's trap re-executes on the thaw (its `sepc` is not
  advanced). The BKL serializes the decision: while the ordering hart holds it, the target is
  in user mode (the IPI lands), waiting for the BKL at a trap entry (the mark is seen first) or
  fully saved (the funnel runs here). Nothing waits.
- **A send that would park on a frozen receiver is refused.** Every receive endpoint of a frozen
  process is marked; a send into a marked endpoint whose queue is full returns `Frozen`
  (`EDEADLK`, 35) instead of parking, and an `ipc_call` to a marked endpoint returns `Frozen`
  at once. What fits the queue is taken and delivered on the thaw. Nothing in the system can
  deadlock on a freeze it did not order.
- **An ordered kill is not a crash.** The parent records the pid before the order and the reap
  reads the kernel's `Killed` as the carried-out order: no crash report, no minidump
  (`SELFTEST: task kill ok`). A `Killed` the parent did not order stays a crash.
- **Out of scope here:** thread-wise orders (a thread is not a lifecycle unit), notifying the
  child before a kill (the window's snapshot is the user-facing state, 6e), the policy of WHEN
  to freeze or kill (RFC-0111 §4, 6d-2/6e), memory pressure bands (6e).

## Consequences

- **Positive**: a crashed or killed service leaves no space behind (the board's `mappers=3`
  leak is gone); the lifecycle layer has a real freeze — a hidden app costs no CPU, not by its
  cooperation but by the kernel's — and a kill that reaps whole; no sender can hang on a frozen
  app; the refusal matrix is a boot proof on every lane and the board.
- **Negative / accepted cost**: three syscalls and one errno widen the ABI (`nexus-abi`
  `task_suspend`, `task_kill`, `exec_vmo(…, killable)`, `IpcError::Frozen`); every
  `exec_vmo` caller states `killable`; a frozen task's pending syscall is re-executed after the
  thaw, so a time-based syscall (a timeout) measures the freeze — acceptable, since a frozen
  process is not supposed to observe time passing.
- **Follow-ups**: 6d-2 (the lifecycle contract windowd → abilitymgr → execd that issues the
  orders, ADR-0083); 6e (the kernel's last resort under memory pressure, honouring `killable`).

## Alternatives considered

- **Thread-wise freeze/kill (the task stays the unit).** Rejected: a half-frozen process is a
  deadlock machine (a frozen thread holding a lock a running thread needs), and a half-killed
  one runs in a space being reclaimed. No production system does this.
- **A kill that waits for the running thread to leave the kernel.** Rejected: nothing in this
  kernel waits (RFC-0033); the pending mark + IPI delivers within IPI latency and never blocks
  the orderer.
- **Let a send park on a frozen receiver and wake it at the thaw.** Rejected: the sender's
  own deadline would then measure a freeze it did not order, and a frozen receiver that is
  killed instead of thawed leaves a sender parked on a dead endpoint until the EOF scan — the
  class of hang RFC-0079 removed. Refusing at once is what the mobile systems converged on.
- **Cooperative sleep (the app parks itself when told).** Rejected as the only mechanism: an
  app that does not cooperate (a bug, or a spin) costs the foreground its frames; the kernel's
  freeze is the floor the policy stands on.
