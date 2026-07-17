# ADR-0046: nexus-workpool — deterministic same-AS parallel compute

## Status

Accepted (2026-07-17). Implemented and boot-proven (SMP=1 + SMP=2 marker
gates).

## Context

TASK-0276 mandates ONE process-wide, deterministic thread pool instead of
ad-hoc threads per subsystem, and TASK-0277 forbids lock-free experiments in
first versions. Phase C added the kernel primitives this needs: same-AS
thread spawn (`SYSCALL_AS_SELF` + suspended spawn), fence syscalls, and the
scheduling ABI.

## Decision

`source/libs/nexus-workpool/` is THE process compute pool:

- **Thread model**: fixed worker count (≤ `MAX_WORKERS`), spawned suspended
  into the caller's address space with an **empty capability table** —
  compute-only by construction. The parent transfers exactly two fence caps
  (job, done) before resume. Workers self-pin (worker idx → CPU idx,
  best-effort) via the sched ABI.
- **Coordination**: a job is `(fn, ctx, total)` + a sequence number. Workers
  park in `fence_wait(job, seq)`; the parent publishes the job then signals;
  the LAST finishing worker signals the done fence. No busy-spin, no queues.
- **Determinism contract**: `chunk_bounds(total, workers, idx)` is pure —
  chunk boundaries depend only on the inputs, so for pure per-element jobs
  `workers = 1 ≡ workers = N` (proven by a host equality matrix with real
  threads and by QEMU markers).
- **Bounded everything, fail closed**:
  - depth-1 submission (`Busy` while a job is in flight);
  - a run TIMEOUT **poisons** the pool: worker state is unknown, so every
    later call gets `Poisoned` and uses its inline fallback. The timed-out
    call itself gets `Timeout` — its job DID start, so the caller must NOT
    re-run the same work inline (double-execution hazard);
  - worker stacks are static and carry a **canary at the stack floor**,
    checked after every run — an overflow is a loud `StackOverflow` +
    poison, never silent .bss corruption (the known three-stack-cliff
    failure mode);
  - every outcome is a `#[must_use]` typed error.

## Consequences

- Services get parallelism without owning thread lifecycles; pinched
  (ADR-0045) is the primary consumer.
- A poisoned pool degrades to inline compute for the process lifetime —
  honest (`workers = 0` reporting) but slower; restart clears it.
- Worker identity inside a job is derived from the deterministic chunk
  start; per-worker counters are the dispatch evidence for proofs.

## How to use and extend

- **Use**: `init(workers)` once; `run(total, job_fn, ctx, deadline_ns)` per
  batch. Jobs must write only their own chunk's outputs (share via atomics
  or disjoint ranges). Always handle the error: `Poisoned`/`Busy`/`NotReady`
  → inline fallback is safe; `Timeout` → abort the job, never re-run.
- **Extend**: per-job deadlines beyond the caller-supplied one, more workers
  (raise `MAX_WORKERS` and widen the counters), pool reset after poison
  (requires proving workers are really parked — do not add without a proof),
  worker QoS classes via the sched ABI.
- **Do not**: hand out caps to workers beyond the two fences, build a second
  pool per subsystem, or replace the locked coordination with lock-free
  structures without a dedicated proof budget (TASK-0277).

## References

- ADR-0045 (pinched), ADR-0047 (nexus-inet), ADR-0048 (integrity detectors)
- TASK-0276, TASK-0277; `source/libs/nexus-workpool/src/pool.rs`
