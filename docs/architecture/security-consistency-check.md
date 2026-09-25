# Security Consistency Check (SMP/QoS/Parallelism Tasks)

**Created**: 2026-01-09  
**Purpose**: Ensure no security drifts, duplications, or contradictions across SMP/QoS/parallelism tasks

---

## Overview

This document tracks security invariants across related tasks to prevent:

- ❌ **Drifts**: Contradictory security requirements
- ❌ **Duplications**: Same security checks implemented differently
- ❌ **Gaps**: Missing security coverage

This is a **living consistency checklist**, not an audit certificate. Where tasks intentionally leave options
open, this document records them as **Decision Points** to be resolved before implementation.

---

## Task Dependency Graph

```text
TASK-0011 (Kernel Simplification)
    ↓ (text-only prep)
TASK-0011B (Rust Idioms)
    ↓ (ownership clarity)
TASK-0012 (SMP v1: Per-CPU + IPIs)
    ├─→ TASK-0013 (QoS ABI + timed)
    ├─→ TASK-0042 (SMP v2: Affinity + Shares)
    ├─→ TASK-0247 (RISC-V SMP + HSM/IPI)
    └─→ TASK-0277 (SMP Policy)
         └─→ TASK-0276 (Userspace Parallelism)
```

---

## Security Invariants Matrix

### 1. Per-CPU Ownership (Kernel)

| Task | Invariant | Status |
|------|-----------|--------|
| **TASK-0011B** | Document ownership model (per-CPU scheduler) | ✅ Defined |
| **TASK-0012** | Per-CPU isolation (no cross-CPU mutable access) | ✅ Consistent |
| **TASK-0277** | Per-CPU ownership by default (policy) | ✅ Consistent |
| **TASK-0247** | Per-hart timer isolation (RISC-V specific) | ✅ Consistent |

**Consistency**: ✅ All tasks agree on per-CPU ownership model.

---

### 2. IPI Authentication

| Task | Invariant | Status |
|------|-----------|--------|
| **TASK-0012** | IPI sender is hardware CPU ID (unforgeable) | ✅ Defined |
| **TASK-0277** | No IPI attacks (bounded queues) | ✅ Consistent |
| **TASK-0247** | IPI sender validation (hardware hart ID) | ✅ Consistent |

**Consistency**: ✅ All tasks agree on IPI authentication via hardware ID.

---

### 3. QoS Escalation Prevention

| Task | Invariant | Status |
|------|-----------|--------|
| **TASK-0012** | QoS ordering preserved during work stealing | ✅ Defined |
| **TASK-0013** | Only privileged services can set QoS class | ✅ Consistent |
| **TASK-0042** | Privileged setting for affinity/shares | ✅ Consistent |

**Consistency**: ✅ All tasks agree on privileged QoS setting (via `execd`/`policyd`).

---

### 4. Work Stealing Bounds

| Task | Invariant | Status |
|------|-----------|--------|
| **TASK-0012** | Work stealing bounded (max N tasks per steal) | ✅ Defined |
| **TASK-0042** | Work stealing respects affinity masks | ✅ Consistent |
| **TASK-0277** | Simple steal policy (no complex load balancing) | ✅ Consistent |

**Consistency**: ✅ All tasks agree on bounded work stealing with affinity respect.

---

### 5. Bounded Resources

| Task | Invariant | Status |
|------|-----------|--------|
| **TASK-0012** | IPI mailboxes bounded (prevent DoS) | ✅ Defined |
| **TASK-0013** | Timer registrations bounded (per-task limit) | ✅ Consistent |
| **TASK-0276** | Thread pools bounded (fixed worker count) | ✅ Consistent |
| **TASK-0277** | No heap growth in hot paths | ✅ Consistent |

**Consistency**: ✅ All tasks agree on bounded resources (no unbounded queues/allocations).

---

### 6. Deterministic Proofs

| Task | Invariant | Status |
|------|-----------|--------|
| **TASK-0012** | KSELFTEST markers validate results, not timing | ✅ Defined |
| **TASK-0013** | Coalescing tests use discrete markers, not RTT | ✅ Consistent |
| **TASK-0276** | Parallelism tests: workers=1 vs N equivalence | ✅ Consistent |
| **TASK-0277** | Deterministic proofs validate invariants | ✅ Consistent |

**Consistency**: ✅ All tasks agree on deterministic proofs (no timing-dependent tests).

---

## Security Policy Hierarchy

### Kernel (Privileged)

```text
┌─────────────────────────────────────────┐
│ Kernel (S-mode)                         │
│ ├─ Per-CPU Scheduler (TASK-0012)       │
│ ├─ IPI Handler (TASK-0012)             │
│ ├─ QoS Syscalls (TASK-0013)            │
│ ├─ Affinity Syscalls (TASK-0042)       │
│ └─ SBI HSM/IPI (TASK-0247, RISC-V)     │
└─────────────────────────────────────────┘
         ↓ (syscall boundary)
```

### Userspace (Privileged Services)

```text
┌─────────────────────────────────────────┐
│ execd (spawner)                         │
│ ├─ Applies QoS from recipe configs     │
│ ├─ Applies affinity from recipe configs│
│ └─ Validates via policyd                │
└─────────────────────────────────────────┘
         ↓ (IPC)
┌─────────────────────────────────────────┐
│ policyd (policy authority)              │
│ ├─ Gates QoS escalation (PerfBurst)    │
│ ├─ Gates affinity changes (other tasks)│
│ └─ Audits QoS/affinity changes          │
└─────────────────────────────────────────┘
         ↓ (IPC)
┌─────────────────────────────────────────┐
│ timed (timer coalescing service)       │
│ ├─ Enforces per-task timer limits       │
│ ├─ Coalesces based on QoS class        │
│ └─ No precise timing for untrusted tasks│
└─────────────────────────────────────────┘
```

### Userspace (Unprivileged Apps)

```text
┌─────────────────────────────────────────┐
│ User Apps                               │
│ ├─ Can set own QoS (within recipe)     │
│ ├─ Can set own affinity (within recipe)│
│ ├─ Cannot set QoS/affinity for others  │
│ └─ Cannot escalate beyond recipe limits │
└─────────────────────────────────────────┘
```

---

## Current consistency assessment

✅ **Mostly consistent**: The tasks align on per-CPU ownership, bounded resources, and deterministic proofs.

⚠️ **Decision points remain** (see below): Some areas are deliberately under-specified (authority boundaries,
IPI classes), and must be decided explicitly to prevent later drift.

---

## Potential duplications (NONE FOUND)

✅ **No duplications detected**. Each task has clear scope:

- **TASK-0012**: SMP baseline (per-CPU, IPIs, work stealing)
- **TASK-0013**: QoS ABI + timer coalescing
- **TASK-0042**: Affinity + shares (extends TASK-0012)
- **TASK-0247**: RISC-V SMP specifics (SBI HSM/IPI)
- **TASK-0277**: SMP policy (rules for all SMP work)
- **TASK-0276**: Userspace parallelism (thread pools)

---

## Potential Gaps (IDENTIFIED)

### Gap 1: CPU Affinity Capability Definition

**Issue**: TASK-0042 mentions `CAP_SCHED_SETAFFINITY` capability, but it's not defined in TASK-0011 or kernel capability model.

**Recommendation**:

- Define `CAP_SCHED_SETAFFINITY` in `source/kernel/neuron/src/cap/mod.rs`
- Add to capability kinds enum
- Document in RFC-0005 (Kernel IPC/Capability Model)

**Priority**: Medium (needed for TASK-0042 implementation)

---

### Gap 2: QoS Class Enum Stability

## Decision Points (MUST resolve before implementation)

### Decision 1 — QoS/Affinity authority model (self-modify vs privileged-only)

Tasks currently imply two compatible-but-different interpretations:

- **Privileged-only**: QoS/affinity/shares set only by `execd` gated by `policyd`.
- **Self-modify within limits**: tasks may set their own QoS/affinity within recipe limits.

**Recommended rule (security + pragmatism)**:

- Unprivileged tasks may **only degrade** their own scheduling (e.g., Normal→Idle), not escalate.
- Any escalation (Interactive/PerfBurst, shares > default, pinning) is **privileged** via `execd` + `policyd`
  (deny-by-default).
- Changing another task always requires an explicit capability (e.g., `CAP_SCHED_SETAFFINITY`).

This keeps consumer-OS pragmatism while preventing QoS escalation abuse.

### Decision 2 — IPI classes for rate limiting (correctness vs best-effort)

Rate limiting must not break correctness:

- **Correctness IPIs** (e.g., TLB shootdown) MUST NOT be dropped; they may be merged/coalesced.
- **Best-effort IPIs** (e.g., resched nudges) may be throttled or dropped under caps.

Source-of-truth: `docs/architecture/smp-ipi-rate-limiting.md`.

**Status update**: Closed for v1.

- `QosClass` is now documented with stable wire values under `#[repr(u8)]` in `source/libs/nexus-abi/src/lib.rs`.
- Invalid wire values are deterministically rejected (no clamp), aligned with TASK-0013/RFC-0023 contract language.
- Remaining follow-up: add dedicated ABI layout regression tests in the ABI crate test surface.

**Priority**: Follow-up hardening (non-blocking for TASK-0013 v1 closure)

---

### Gap 3: IPI Rate Limiting Implementation

**Issue**: TASK-0247 mentions IPI rate limiting (1000/sec) but doesn't specify where it's enforced.

**Recommendation**:

- Add IPI rate limiter to kernel IPI handler
- Use per-CPU atomic counter (reset every second)
- Reject IPIs exceeding limit with -EBUSY

**Priority**: Medium (DoS mitigation)

---

### Gap 4: Virtio-blk Signature Verification

**Issue**: TASK-0247 mentions packagefs signature verification but doesn't specify format or implementation.

**Recommendation**:

- Link to TASK-0008 (Security Hardening v1) for policy authority + audit baseline and signature decision flow
- Use Ed25519 signatures (consistent with device keys)
- Verify signature before mounting packagefs

**Priority**: High (security-critical)

---

## Action Items

### Immediate (Post TASK-0012 Baseline)

1. ✅ Add security sections to all SMP/QoS tasks (DONE)
2. ✅ Complete TASK-0012 deterministic SMP baseline (DONE; anti-fake IPI proof chain + `test_reject_*`)
3. 🔄 Define `CAP_SCHED_SETAFFINITY` in kernel capability model
4. ✅ Finalize QoS enum ABI stability in TASK-0013 ABI surfaces
5. 🔄 Specify full token-bucket/global IPI limiter implementation for TASK-0042

### Before TASK-0042 (SMP v2)

1. 🔄 Implement full IPI rate limiting in kernel
2. 🔄 Add affinity capability checks to syscall handlers

### Before TASK-0247 (RISC-V SMP)

1. 🔄 Specify virtio-blk signature verification format
2. 🔄 Link to TASK-0008B for device-key entropy/keygen when signature material depends on real OS keys

---

## Related Documents

- `tasks/TASK-0011-kernel-simplification-phase-a.md` — Kernel headers + invariants
- `tasks/TASK-0011B-kernel-rust-idioms-pre-smp.md` — Rust ownership model
- `tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md` — SMP baseline
- `tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md` — QoS + timed
- `tasks/TASK-0042-smp-v2-affinity-qos-budgets-kernel-abi.md` — Affinity + shares
- `tasks/TASK-0247-bringup-rv-virt-v1_1b-os-smp-hsm-ipi-blkd-packagefs-selftests.md` — RISC-V SMP
- `tasks/TASK-0277-kernel-smp-parallelism-policy-v1-deterministic.md` — SMP policy
- `tasks/TASK-0276-parallelism-v1-deterministic-threadpools-policy-contract.md` — Userspace parallelism
- `docs/architecture/16-rust-concurrency-model.md` — Rust concurrency patterns
- `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md` — Capability model

---

## Conclusion

✅ **Security consistency is GOOD**. All tasks have:

- Consistent security invariants (no contradictions)
- Clear scope boundaries (no duplications)
- Comprehensive threat models (minimal gaps)

**Next steps**:

1. Address identified gaps (CAP_SCHED_SETAFFINITY, QoS ABI finalization, full IPI rate limiting)
2. Use TASK-0012 as fixed baseline for TASK-0013 and TASK-0042 follow-ups
3. Keep SMP proofs deterministic (`REQUIRE_SMP=1` for SMP marker ladder) and preserve anti-fake semantics

**Overall assessment**: ✅ **Ready to proceed** with post-SMP-baseline tasks.
