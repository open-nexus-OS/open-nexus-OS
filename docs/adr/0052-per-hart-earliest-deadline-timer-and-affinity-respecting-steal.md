# ADR-0052: Per-hart earliest-deadline timer arming + affinity-respecting steal park

- Status: Accepted
- Date: 2026-07-20
- Related: ADR-0049 (BKL lock classes + soft-RT CPU placement), RFC-0033 (soft-real-time spine), TASK-0042 (affinity ABI)

## Context

Each hart has ONE timer compare register (mtimecmp via SBI), but many independent
kernel paths arm it through `Timer::set_wakeup`: timer caps (`sys_timer_set`),
timed IPC recv/send, `waitset_wait`, and `fence_wait`. `set_wakeup` blindly
overwrites the compare value, and the timer-IRQ re-arm in
`process_expired_timers` considered ONLY timer-cap deadlines (else a 10 ms
fallback). Consequence: windowd's 8.33 ms display pacer (a one-shot timer cap)
could be clobbered by any later, longer deadline armed on the same hart — the
wake then slipped to the 100 Hz fallback tick. Stage-0 diagnostics
(`windowd: loop hz … slip=`) measured exactly this under SMP=4 drag load:
the ≥8 ms slip bucket dominated (e.g. 22 of 25 pacer ticks a full frame late).

Independently, the work-stealing path violated declarative CPU placement
(ADR-0049): `steal_into_current` parks an affinity-REJECTED task at the front
of **cpu0's** queue, and `schedule_next` performs no affinity check — so
background (cpu1-3) work could run on cpu0 and steal display-chain time.

## Decision

1. **Earliest-deadline arming (`arm_wakeup`).** A per-hart atomic shadow of the
   armed deadline; `arm_wakeup(timer, deadline)` programs the compare register
   only when the requested deadline is EARLIER than the armed one. All syscall
   arming sites (timer caps, IPC recv/send, waitset, fence) go through it.
   **Self-heal invariant:** the shadow suppresses an arm only while it is
   still PENDING (`armed > now`). Not every timer-IRQ path clears the shadow
   (an S-mode trap re-arms without running `process_expired_timers`), so an
   elapsed shadow must never read as "earlier" — the first implementation
   without this check silenced the hart's timer entirely (boot-time windowd
   present-NACK storms).
2. **Re-arm across ALL deadline sources.** `process_expired_timers` clears the
   shadow, delivers expiries, then re-arms to the true earliest of: timer-cap
   deadlines AND every blocked task's IPC/waitset/fence deadline (a bounded
   read-only scan of the task table — the same table the expiry wakers already
   iterate), falling back to the 10 ms heartbeat only when nothing is pending.
   The fallback stays armed at all times (missed-wakeup safety net unchanged).
3. **Steal park respects affinity homes.** A steal-rejected task is parked at
   the front of its **home CPU's** queue (home resolved by the caller, which
   owns the `TaskTable`), not cpu0's — closing the placement leak without
   touching the `schedule_next` hot path.

No syscall ABI, wire format, or userspace contract changes.

## Consequences

- windowd's 120 Hz pacer deadline survives every later arm on the same hart;
  cadence jitter from timer clobbering is eliminated by construction (proof:
  the Stage-0 slip histogram must collapse into the <1 ms bucket during drag,
  modulo genuine cpu0 compute load).
- The IRQ re-arm scan is O(tasks) under the BKL — same bound as the existing
  `wake_expired_ipc_deadlines` walk in the same handler; no new lock class.
- Harts may arm for another hart's blocked task (global scan): harmless — the
  expiry walk is already global and cross-hart wakes IPI immediately.
- Affinity masks are now enforced on the steal-park path; cpu0 keeps its
  display-chain reservation even when its queue momentarily drains.
