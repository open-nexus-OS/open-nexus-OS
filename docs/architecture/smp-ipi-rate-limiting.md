# SMP IPI Rate Limiting Policy

**Created**: 2026-01-09  
**Owner**: @kernel-smp-team  
**Status**: Active policy; TASK-0012 baseline invariants implemented, TASK-0042 budget layer pending  
**Related**: TASK-0012 (SMP v1), TASK-0042 (SMP v2), SECURITY-CONSISTENCY-CHECK.md

---

## Overview

Inter-Processor Interrupts (IPIs) are a privileged kernel mechanism for cross-CPU coordination.
Without rate limiting, a malicious or buggy task could trigger excessive IPIs, causing:

- **DoS**: CPU starvation on target cores
- **Information leakage**: Timing side-channels via IPI latency
- **Scheduler thrashing**: Excessive work stealing attempts

This document defines the **rate limiting policy** for NEURON's SMP implementation.

---

## Threat Model

### Attacker Capabilities

- **Unprivileged task**: Can request CPU affinity changes (within recipe limits)
- **Privileged task**: Can set affinity for other tasks (with `CAP_SCHED_SETAFFINITY`)

### Attack Vectors

1. **IPI flood**: Task rapidly changes affinity to trigger resched IPIs
2. **Work stealing abuse**: Task yields repeatedly to trigger steal attempts
3. **Cross-CPU ping-pong**: Two tasks coordinate to bounce between CPUs

---

## Rate Limiting Strategy

### 0. IPI Classes (Correctness vs. Best-effort)

Not all IPIs are equal. For a secure kernel, we must distinguish:

- **Correctness IPIs (MUST NOT DROP)**: required to preserve memory safety / isolation invariants.
  - Examples: **TLB shootdown**, address-space / ASID-related invalidations, page-table coherency events.
  - Policy: may be **coalesced/merged**, but must not be dropped. If overloaded, the system must apply
    backpressure (delay caller, merge requests) rather than silently skipping.

- **Best-effort IPIs (MAY DROP / THROTTLE)**: used to improve responsiveness but not required for correctness.
  - Examples: **Resched/“please reschedule”**, opportunistic nudge signals.
  - Policy: can be rate-limited and even dropped under global caps (with bounded audit).

This split is required so rate limiting does not accidentally create stale-mapping bugs.

### 1. Per-Task IPI Budget (TASK-0042)

```rust
pub struct TaskIpiState {
    /// Number of IPIs triggered by this task in current window
    ipi_count: u32,
    /// Window start time (ns)
    window_start_ns: u64,
    /// Max IPIs per window (from recipe or default)
    max_ipis_per_window: u32,
}

const DEFAULT_IPI_WINDOW_NS: u64 = 100_000_000; // 100ms
const DEFAULT_MAX_IPIS_PER_WINDOW: u32 = 100;
```

**Enforcement**:

- When task requests affinity change → check budget
- If `ipi_count >= max_ipis_per_window` → reject with `EBUSY`
- Reset counter every `DEFAULT_IPI_WINDOW_NS`

### 2. Global IPI Rate Limiter (Kernel-Wide)

```rust
pub struct GlobalIpiLimiter {
    /// Total IPIs sent in current window (all CPUs)
    total_ipis: AtomicU64,
    /// Window start time
    window_start_ns: u64,
    /// Hard cap (prevents kernel-wide DoS)
    max_ipis_per_window: u64,
}

const GLOBAL_IPI_WINDOW_NS: u64 = 10_000_000; // 10ms
const GLOBAL_MAX_IPIS_PER_WINDOW: u64 = 10_000;
```

**Enforcement**:

- Before sending any **best-effort** IPI → increment `total_ipis`
- If `total_ipis >= max_ipis_per_window` → drop **best-effort** IPI, log bounded warning
- Reset counter every `GLOBAL_IPI_WINDOW_NS`

**Correctness IPIs** are excluded from the “drop” path:

- They must be **merged/coalesced** (e.g., per-target “pending shootdown” bit + latest ASID/range),
  and delivered at the next safe point.

### 3. Work Stealing Rate Limiting (TASK-0012)

```rust
pub struct PerCpuStealState {
    /// Last steal attempt timestamp
    last_steal_ns: u64,
    /// Minimum interval between steal attempts
    min_steal_interval_ns: u64,
}

const DEFAULT_MIN_STEAL_INTERVAL_NS: u64 = 1_000_000; // 1ms
```

**Enforcement**:

- Before attempting work steal → check `last_steal_ns`
- If `(now_ns - last_steal_ns) < min_steal_interval_ns` → skip steal
- Update `last_steal_ns` after successful steal

---

## Recipe Configuration (TASK-0042)

Services can request higher IPI budgets via recipe:

```toml
[runtime.smp]
max_ipis_per_window = 500  # Higher budget for latency-sensitive services
ipi_window_ms = 100
```

**Validation**:

- `max_ipis_per_window` ≤ 10,000 (hard cap)
- `ipi_window_ms` ≥ 10 (minimum window)

---

## Security Invariants

### MUST Hold

1. **Per-task budget enforced**: No task can exceed `max_ipis_per_window`
2. **Global cap enforced**: Total IPIs never exceed `GLOBAL_MAX_IPIS_PER_WINDOW`
3. **No unbounded loops**: Work stealing has minimum interval
4. **Correctness IPIs never dropped**: shootdowns/invalidation IPIs are merged but not skipped

### DON'T DO

- ❌ DON'T allow unlimited IPIs even for privileged tasks
- ❌ DON'T skip rate limiting in "fast path" (always check)
- ❌ DON'T use wall-clock time (use monotonic kernel timer)
- ❌ DON'T treat shootdown IPIs as best-effort (stale mappings become security bugs)

---

## Testing Requirements (TASK-0012, TASK-0042)

### Unit Tests

```rust
#[test]
fn test_reject_ipi_flood() {
    // Task triggers 101 affinity changes in 100ms
    // → First 100 succeed, 101st fails with EBUSY
}

#[test]
fn test_global_ipi_cap() {
    // 10,001 IPIs in 10ms window
    // → 10,001st IPI is dropped
}
```

### QEMU Markers

```text
SELFTEST: ipi_rate_limit ok
SELFTEST: ipi_flood_reject ok
```

---

## Implementation Phases

### Phase 1 (TASK-0012): Basic Rate Limiting

- ✅ Bounded resched mailbox semantics (no unbounded queues/loops)
- ✅ Bounded work-steal attempts per scheduling tick
- ✅ Deterministic anti-fake IPI proof chain + counterfactual marker (`ipi counterfactual ok`)
- ⚠️ Full global IPI token-bucket style limiter remains follow-up work (TASK-0042)

### Phase 2 (TASK-0042): Per-Task Budgets

- 🔄 Per-task IPI state
- 🔄 Recipe-based budget configuration
- 🔄 Reject with `EBUSY` on budget exhaustion

---

## Audit Logging

When rate limits are hit:

```text
AUDIT: IPI rate limit hit: task=<pid> budget=<max> window_ns=<window>
AUDIT: Global IPI cap hit: total=<count> window_ns=<window>
```

**DO NOT LOG**:

- Task names (use PID only)
- Exact timing (use window boundaries)

---

## References

- **TASK-0012**: SMP v1 (basic IPI + work stealing)
- **TASK-0042**: SMP v2 (affinity + per-task budgets)
- **TASK-0277**: SMP policy (determinism rules)
- **docs/standards/SECURITY_STANDARDS.md**: Rate limiting principles
