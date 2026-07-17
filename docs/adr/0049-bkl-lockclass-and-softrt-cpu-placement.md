# ADR-0049: BKL lock-classes + soft-realtime CPU placement (SMP=4 interactive)

## Status

Accepted (2026-07-17). Implemented and boot-proven; gated by
`KSELFTEST: bkl budget ok` in the SMP profile.

## Context

With real parallel harts (MTTCG), the v1 BKL serialized EVERY syscall: the
pinned-to-nothing UI hotpath queued behind background work. Measured on the
SMP=2 gate: BKL waits up to **90.8ms**, single holds up to **90ms**
(`nr=5` vmo_create zeroing 4MB, `nr=13/17` exec ELF copies, `nr=44`
debug_write UART). `just start` with SMP=4 was an interactive regression
(mouse lag, frame jitter, slow app loads).

## Decision

Four declarative mechanisms, all measured by the P0 budget accounting
(`core/trap/budgets.rs` — the budget SSOT — plus per-boot max/histogram
atomics drained into the boot-end gate line):

1. **Declarative CPU placement** (`service_topology::affinity_for`, ONE
   const-fn SSOT, host-tested): display/input chain (gpud, windowd, inputd,
   hidrawd, touchd) → cpu0; background services → cpu1–3; applied by init at
   resume time via the existing cross-task sched ABI (init joined the
   declarative QoS-admin list). The kernel clamps masks to online CPUs, so
   SMP=1 degrades transparently.
2. **Phased syscall class** (`LockClass` idea): `vmo_create`, `exec`,
   `exec_v2` run their expensive byte-moving middle with the BKL **dropped**
   (trap handler routes them: phase A reserves/maps under the BKL and STAGES
   copy/zero ops into a bounded `CopyPlan`; phase B executes the plan
   unlocked; phase C installs the result). Safety: reserved ranges are
   unreachable until the result becomes visible (vmo caps install in phase C;
   exec'd tasks spawn suspended and resume only after the syscall returns),
   and the serving hart cannot be preempted (SIE off in trap context).
3. **Lock-free syscall class** (`api::lockfree_syscall`): pure UART/time
   syscalls (`debug_putc`, `debug_write`, `nsec`) are served before any
   kernel lock is taken.
4. **cpu0 right-of-way at the BKL**: while cpu0 (the soft-RT hart) is
   waiting, other harts back off (bounded, no starvation) so cpu0 takes the
   next release instead of joining a convoy.
5. Supporting allocator work: the VMO arena gained a **zero-frontier**
   (idle harts pre-zero ahead of the bump pointer and scrub freed ranges
   from a dirty list into the clean free list), so allocation-time zeroing
   is the exception, not the rule.

Result on the SMP=2 gate: max wait 90.8ms → **~6ms**, max hold 90ms →
**~4ms**, zero >10ms convoy events. Budgets are calibrated for MTTCG
(8ms wait / 5ms hold ≈ ≤160µs/100µs at a ~50× emulation factor) and enforced
by the gate — regressions fail the boot, not the user's mouse.

## Consequences

- SMP=4 + MTTCG is the interactive `just start` default again; SMP=1/icount
  remains the deterministic proof profile.
- The phased/lock-free tables are the declarative seam: moving another
  syscall out of the BKL means adding it to a match arm with a documented
  safety argument, not restructuring the handler.
- The BKL still exists; a full router/scheduler lock split (P3) stays a
  budget-gated follow-up — pursued only if the gate regresses as workloads
  grow.

## How to extend

- New expensive syscall: prefer STAGING (CopyPlan) over new locks; prove the
  unreachability argument, add the nr to the phased match, keep the budget
  gate green.
- New latency class (e.g. audio): extend `affinity_for` and, if needed, add
  a right-of-way flag per hart class — both declarative.

## References

- ADR-0045/0046/0048; `core/trap/budgets.rs`, `core/trap/handler.rs`
  (`phased_syscall`, lock-free class), `core/trap/runtime.rs` (right-of-way),
  `syscall/api/vmo.rs` (zero-frontier), `service_topology.rs`
  (`affinity_for`).
