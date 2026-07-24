# TASK-0304: Thread teardown on process exit — shared-AS refcount lifecycle (seed)

- Status: Seed (not started)
- Owners: @kernel-team
- Related: TASK-0303 (process reaper), RFC-0081, TASK-0276 (compute-only threads)

## Problem

A process that spawns threads via `SYSCALL_SPAWN(as_self)` shares ONE address
space: `attach` adds each thread pid to the AS `owners` set. `sys_exit` on the
main task does NOT terminate its sibling threads, so the AS `refcount` never
reaches 0 → `reap_child`'s `destroy` fails with `InUse` and the page-table heap
leaks. Observed today ONLY for the kernel **workpool selftest**
(`TASK: destroy as failed pid=48 err=InUse`, right before `SELFTEST: thread
spawn ok`); no shipping service spawns threads, so it is bounded and not the
app-launch pressure that TASK-0303 fixes.

## Why it was split out of TASK-0303

The correct fix requires terminating threads that may be **running on other
harts** under SMP. There is no race-free "kill a task executing on another CPU"
primitive today — doing it wrong risks tearing an AS out from under a running
thread. That is genuine SMP-quiescence design (IPI + acknowledge + stack
reclaim) and an ADR (kernel task-lifecycle contract), i.e. multi-day. Kept
separate so the load-bearing reaper (TASK-0303) can land now.

## Candidate designs (to evaluate)

1. **Detach-on-own-exit + destroy-on-last-detach**: every task (thread or
   leader) detaches its own AS reference in `exit_current`; the AS is destroyed
   when the last owner detaches, regardless of order. No cross-hart kill, but
   orphan threads must still be reaped to exit — needs an orphan-adoption path.
2. **Leader-exit terminates siblings**: on process exit, actively stop every
   task sharing the AS (cross-hart IPI + quiesce), then destroy. Strongest
   semantics; hardest (SMP safety).
3. **Deferred-destroy list**: reap parks the AS on a bounded "pending destroy"
   list; the last thread detach triggers the actual destroy. Self-healing IF
   threads eventually exit; needs the detach hook from (1).

## Proof (when taken)

Host: refcount reaches 0 across leader/thread exit in any order; `destroy`
succeeds. QEMU: `destroy as failed InUse` disappears from the selftest boot;
AS live-count returns to baseline after the workpool selftest.
