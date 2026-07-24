# TASK-0304: Shared-AS reap ordering + thread teardown

- Status: Part 1 Done (reap ordering, boot-proven 2026-07-24); Part 2 deferred
- Owners: @kernel-team
- Related: TASK-0303 (process reaper), RFC-0081, TASK-0276 (compute-only threads)

## What the `destroy as failed InUse` actually was (investigated 2026-07-24)

The boot log's `TASK: destroy as failed pid=48 err=InUse` (right before
`SELFTEST: thread spawn ok`) was NOT a leak needing cross-hart thread teardown.
Investigation of the actual thread lifecycle:

- pid 48 is the **thread-spawn selftest** thread
  (`selftest-client .../phases/exec.rs::thread_spawn_proof`): `thread_entry`
  returns → the trampoline exits 0 → the parent (selftest-client) reaps it via
  `wait(pid)`. The thread DOES exit and IS reaped.
- The `InUse` came from `reap_child`: it unconditionally called
  `address_spaces.destroy(handle)` after detaching the reaped task. For a
  **thread**, the handle is the AS **shared** with the still-living parent (and
  the parked workpool worker threads), so `destroy` correctly refused with
  `InUse` — but `reap_child` logged it as an error.

So the address space was never leaked: `detach` released the reaped thread's
reference, and the shared AS is correctly reclaimed when its **last** owner
(the leader) is reaped. The defect was purely reap ordering + a misleading log.

## Part 1 — reap ordering (DONE, boot-proven)

`reap_child` (`task/mod.rs`) now destroys the AS only when the reaped task was
its last owner: `destroy` is attempted and a returned `InUse` is accepted
silently (a co-owner — parent or sibling thread — is still alive; not an error);
any other error is still logged. Invariant covered by the existing target unit
`address_space::tests::destroy_rejects_address_space_with_owner` (destroy
refuses while an owner remains, succeeds once the owners set empties).

- [x] `reap_child` destroys the shared AS only when last owner (`InUse` accepted)
- [x] Boot proof: `ci-os-smp1` green; `TASK: destroy as failed InUse` gone;
      `SELFTEST: thread spawn ok` + `workpool bounded ok` still green (thread
      reaped correctly, AS kept alive for the living parent).

## Part 2 — active teardown of parked daemon threads (DEFERRED, genuinely hard)

The ONE case the refcount model cannot reclaim on its own: a service that spawns
**persistent** worker threads (like `nexus-workpool`, whose workers park on the
job fence forever, `pool.rs`) and then **exits**. Its reap detaches the leader,
but the parked workers still own the AS → it survives until they are terminated.
No shipping service does this today (the workpool's only users — selftest-client,
pinched — are long-lived), so there is no live leak.

When a shipping service needs a restartable worker pool, terminate its sibling
threads on exit. The tractable slice is that workpool workers are **blocked**
(parked on a fence) at their owner's exit, so terminating a blocked task is
race-free (dequeue from the block set → mark dead → detach → free its stack).
The genuinely hard slice — a sibling thread **running on another hart** at exit —
needs cross-hart quiesce (IPI + acknowledge) and an ADR (kernel task-lifecycle
contract). Candidate designs:

1. **Detach-on-own-exit + destroy-on-last-detach**: each task detaches its AS
   reference in `exit_current`; the AS is destroyed when the last owner leaves.
   Needs a safe deferred-destroy context (not while any hart has that SATP).
2. **Leader-exit terminates siblings**: on process exit, stop every task
   sharing the AS (blocked ones directly; running ones via cross-hart quiesce),
   then destroy. Strongest semantics; hardest.

## Proof (Part 2, when taken)

QEMU: a service with a worker pool exits and restarts N times with AS live-count
returning to baseline each cycle (no growth); no `destroy as failed`.
