# Kernel-Touch Task Inventory

**Purpose**: Systematic classification of all tasks with kernel-relevant scope to ensure security consistency, decision point alignment, and no architectural drift.

**Last Updated**: 2026-01-09

---

## Classification Criteria

A task is "kernel-touch" if it:

- Modifies kernel source code (`source/kernel/neuron/`)

- Defines kernel ABI/syscalls (`nexus-abi`, capability model)

- Implements kernel-facing drivers (MMIO, interrupts, DMA)

- Defines SMP/parallelism/scheduling policy

- Implements bring-up/boot that exercises kernel primitives

- Defines security boundaries enforced by kernel capabilities

---

## Inventory Status Summary

| Category                         | Total | With Security Section | Decision Points Aligned | Status            |
|----------------------------------|-------|-----------------------|-------------------------|-------------------|
| **Core Kernel**                  | 8     | 8                     | 8                       | ✅ Complete       |
| **Bring-up/Boot**                | 3     | 3                     | 3                       | ✅ Complete       |
| **Security/ABI**                 | 4     | 4                     | 4                       | ✅ Complete       |
| **Userspace (Kernel-dependent)** | 7     | 7                     | 7                       | ✅ Complete       |
| **Networking (Kernel-gated)**    | 5     | 5                     | 3                       | ⚠️ Needs Review   |
| **Total Kernel-Touch**           | 27    | 27                    | 25                      | 93% Complete      |

---

## Category 1: Core Kernel Tasks

These tasks directly modify kernel code or define kernel architecture.

### ✅ TASK-0011: Kernel Simplification Phase A

- **Status**: Has Security section

- **Decision Points**: Aligned (W^X, Capability rights, Bootstrap integrity)

- **Scope**: Text-only restructuring for SMP debugging

- **Security Invariants**: 7 defined

- **Notes**: Foundation for SMP work

### ✅ TASK-0011B: Kernel Rust Idioms Pre-SMP

- **Status**: Has Security section

- **Decision Points**: Aligned (Ownership model, Newtype construction)

- **Scope**: Rust-specific optimizations before SMP

- **Security Invariants**: 6 defined

- **Notes**: Implements newtypes, Send/Sync markers, error unification

### ✅ TASK-0012: Kernel SMP v1 (Per-CPU Runqueues + IPIs)

- **Status**: Has Security section

- **Decision Points**: Aligned (IPI classes, Per-CPU isolation)

- **Scope**: SMP bring-up with per-CPU runqueues

- **Security Invariants**: 6 defined

- **Threat Model**: Per-CPU data races, IPI spoofing, work stealing attacks

- **Notes**: Core SMP implementation

### ✅ TASK-0013: Perf/Power v1 (QoS ABI + Timed Coalescing)

- **Status**: Has Security section

- **Decision Points**: Aligned (QoS authority model)

- **Scope**: QoS hints and timer coalescing

- **Security Invariants**: 6 defined

- **Threat Model**: QoS escalation, timer coalescing bypass

- **Notes**: Foundation for power management

### ✅ TASK-0042: SMP v2 (Affinity + QoS Budgets + Kernel ABI)

- **Status**: Has Security section

- **Decision Points**: Aligned (QoS/Affinity authority)

- **Scope**: CPU affinity and QoS CPU budgets

- **Security Invariants**: 6 defined

- **Threat Model**: CPU affinity bypass, QoS shares manipulation

- **Notes**: Extends TASK-0012/0013

### ✅ TASK-0267: Kernel IPC v1 (Framed Channels + Capability Transfer)

- **Status**: Has Security section

- **Decision Points**: Aligned (IPC ownership transfer)

- **Scope**: Minimum viable kernel IPC surface

- **Security Invariants**: 7 defined

- **Threat Model**: IPC spoofing, capability leakage, channel exhaustion

- **Notes**: Foundation for all userspace communication

### ✅ TASK-0276: Parallelism v1 (Deterministic Threadpools Policy)

- **Status**: Has Security section

- **Decision Points**: Aligned (Deterministic parallelism)

- **Scope**: Userspace parallelism policy contract

- **Security Invariants**: 5 defined

- **Threat Model**: Resource exhaustion, timing side-channels

- **Notes**: Userspace-facing, kernel-enforced bounds

### ✅ TASK-0277: Kernel SMP Parallelism Policy v1 (Deterministic)

- **Status**: Has Security section

- **Decision Points**: Aligned (SMP determinism)

- **Scope**: v1 rules for all SMP/parallel kernel work

- **Security Invariants**: 5 defined

- **Threat Model**: Data races, IPI attacks, lock ordering deadlocks

- **Notes**: Meta-policy for all SMP kernel work

---

## Category 2: Bring-up/Boot Tasks

These tasks exercise kernel primitives during bring-up and define boot contracts.

### ✅ TASK-0244: Bringup RV virt v1.0a (Host DTB + SBI Shim)

- **Status**: Has Security section

- **Decision Points**: Aligned (DTB validation, SBI shim security)

- **Scope**: Host-first RISC-V virt machine bring-up

- **Security Invariants**: 5 defined

- **Threat Model**: DTB tampering, SBI call injection

- **Notes**: Foundation for RISC-V OS bring-up

### ✅ TASK-0245: Bringup RV virt v1.0b (OS Kernel UART/PLIC/Timer + uartd)

- **Status**: Has Security section

- **Decision Points**: Aligned (UART/PLIC/Timer security)

- **Scope**: OS/QEMU kernel integration for hardware bring-up

- **Security Invariants**: 6 defined

- **Threat Model**: UART injection, PLIC manipulation, timer attacks

- **Notes**: First OS kernel + userspace driver integration

### ✅ TASK-0247: Bringup RV virt v1.1b (OS SMP + HSM/IPI + virtioblkd + packagefs)

- **Status**: Has Security section

- **Decision Points**: Aligned (SBI HSM/IPI security, Virtio-blk integrity)

- **Scope**: RISC-V SMP bring-up with virtio-blk and packagefs

- **Security Invariants**: 6 defined (Phase 1: CRC32, Phase 2: Signature verification)

- **Threat Model**: Hart boot hijacking, SBI HSM abuse, IPI flooding, packagefs tampering

- **Notes**: Critical for SMP + persistent storage

---

## Category 3: Security/ABI Tasks

These tasks define security boundaries and syscall filters.

### ✅ TASK-0010: Device MMIO Access Model v1

- **Status**: Has Security section

- **Decision Points**: Aligned (MMIO capability model)

- **Scope**: Safe userspace MMIO for virtio devices

- **Security Invariants**: 6 defined

- **Threat Model**: Arbitrary memory access, DMA attacks, execute from MMIO

- **Notes**: Foundation for userspace drivers

### ✅ TASK-0188: Kernel Sysfilter v1 (Task Profiles + Rate Buckets)

- **Status**: Has Security section

- **Decision Points**: Aligned (Sysfilter enforcement)

- **Scope**: True kernel-level syscall enforcement

- **Security Invariants**: 7 defined

- **Threat Model**: Syscall filter bypass, profile tampering, DoS

- **Notes**: Kernel-enforced seccomp-like filtering

### ✅ TASK-0019: Security v2 (Userland ABI Syscall Guardrails)

- **Status**: Has Security section ✅

- **Decision Points**: Aligned (Userland guardrails, NOT kernel-enforced)

- **Scope**: Userland syscall filters (not kernel-enforced)

- **Security Invariants**: 5 defined

- **Threat Model**: Bypass via raw ecall, profile tampering, audit evasion

- **Notes**: Explicitly NOT a security boundary; kernel enforcement is TASK-0188

### ✅ TASK-0028: ABI Filters v2 (Argument Matchers + Learn→Enforce)

- **Status**: Has Security section ✅

- **Decision Points**: Aligned (depends on TASK-0019)

- **Scope**: Extends TASK-0019 with argument-level matching

- **Security Invariants**: 5 defined

- **Threat Model**: Bypass via raw ecall, learn mode abuse, regex DoS

- **Notes**: Learn mode for policy generation, not enforcement

---

## Category 4: Userspace (Kernel-dependent)

These tasks are userspace-focused but depend on kernel primitives or define kernel-facing contracts.

### ✅ TASK-0001: Runtime Roles & Boundaries

- **Status**: Done (no Security section needed - architectural)

- **Decision Points**: N/A (defines authorities, not security boundaries)

- **Scope**: Defines init/execd/loader single-authority model

- **Notes**: Architectural foundation, not security-critical

### ✅ TASK-0002: Userspace VFS Proof

- **Status**: Done (no Security section needed - proof task)

- **Decision Points**: N/A (proof task)

- **Scope**: Proves VFS from userspace via real IPC

- **Notes**: Proof task, security handled by underlying services

### ✅ TASK-0008: Security Hardening v1 (nexus-sel + Audit + Device Keys)

- **Status**: Has Security section

- **Decision Points**: Aligned (Policy authority, Channel-bound identity)

- **Scope**: Policy engine, audit trail, keystored hardening

- **Security Invariants**: 7 defined

- **Threat Model**: Policy bypass, privilege escalation, identity spoofing

- **Notes**: Core security enforcement layer

### ⚠️ TASK-0009: Persistence v1 (Virtio-blk + statefs)

- **Status**: Has Security section

- **Decision Points**: Needs alignment (depends on TASK-0010 for MMIO)

- **Scope**: Userspace block device + statefs journal

- **Security Invariants**: 6 defined

- **Threat Model**: Credential theft, data tampering, journal corruption

- **Action Required**: Verify TASK-0010 dependency is explicit

### ✅ TASK-0031: Zero-copy VMOs v1

- **Status**: Has Security section ✅

- **Decision Points**: Aligned (VMO transfer, RO sealing = library convention in v1)

- **Scope**: Shared RO buffers via VMO syscalls

- **Security Invariants**: 5 defined

- **Threat Model**: VMO content tampering, capability leakage, use-after-free

- **Notes**: v1 RO sealing is library convention; kernel enforcement in v2+

### ✅ TASK-0039: Sandboxing v1 (VFS Namespaces + CapFd + Manifest)

- **Status**: Has Security section ✅

- **Decision Points**: Aligned (Userspace confinement, NOT kernel-enforced)

- **Scope**: Userspace sandboxing without kernel changes

- **Security Invariants**: 6 defined

- **Threat Model**: Sandbox escape, CapFd forgery, path traversal, capability bypass

- **Notes**: Userspace confinement for compliant apps; kernel enforcement future task

### ✅ TASK-0228: OOM Watchdog v1 (oomd + Cooperative Memstat)

- **Status**: Has Security section ✅

- **Decision Points**: Aligned (Cooperative memstat, NOT kernel RSS)

- **Scope**: Deterministic memory watchdog

- **Security Invariants**: 5 defined

- **Threat Model**: OOM kill bypass, DoS via fake memstat, kill authority abuse

- **Notes**: Cooperative memstat; kernel RSS accounting is future task

---

## Category 5: Networking (Kernel-gated)

These tasks depend on kernel networking primitives (TASK-0010 MMIO access).

### ✅ TASK-0003B: DSoftBus Noise XK Handshake (no_std)

- **Status**: Has Security section (scope: handshake only)

- **Decision Points**: Aligned (Test keys labeled, identity binding deferred to TASK-0004)

- **Scope**: Noise XK handshake implementation

- **Security Invariants**: 4 defined

- **Threat Model**: MITM during handshake, key compromise, replay

- **Notes**: Identity binding enforcement is TASK-0004 scope

### ✅ TASK-0004: Networking Step 2 (DHCP + ICMP + DSoftBus Dual-Node)

- **Status**: Has Security section

- **Decision Points**: Needs alignment (Identity binding enforcement)

- **Scope**: DHCP, ICMP, discovery-driven sessions, identity binding

- **Security Invariants**: 6 defined

- **Threat Model**: Spoofed discovery, identity confusion, MITM

- **Action Required**: Verify identity binding decision point is documented

### ✅ TASK-0005: Networking Step 3 (Cross-VM DSoftBus + Remote Proxy)

- **Status**: Has Security section

- **Decision Points**: Needs alignment (Remote gateway authority)

- **Scope**: Cross-VM sessions + remote samgr/bundlemgr proxy

- **Security Invariants**: 6 defined

- **Threat Model**: Cross-VM session hijacking, unauthorized remote access

- **Action Required**: Verify remote gateway authority model is documented

### ✅ TASK-0006: Observability v1 (logd + nexus-log + Crash Reports)

- **Status**: Has Security section

- **Decision Points**: Aligned (Channel-bound identity for log records)

- **Scope**: logd journal + structured logging

- **Security Invariants**: 5 defined

- **Threat Model**: Information disclosure, log injection, log tampering

- **Notes**: Foundation for audit trail

### ✅ TASK-0007: Updates & Packaging v1.1 (A/B Skeleton + System-set Index)

- **Status**: Has Security section

- **Decision Points**: Aligned (Bundle manifest contract, Signature verification)

- **Scope**: Userspace A/B skeleton + signed system-set index

- **Security Invariants**: 7 defined

- **Threat Model**: Malicious update injection, signature bypass, rollback attacks

- **Notes**: Foundation for OTA security

---

## Decision Points Requiring Cross-Task Alignment

### 1. QoS/Affinity Authority Model

**Status**: ✅ Documented in `docs/architecture/SECURITY-CONSISTENCY-CHECK.md`

**Decision**: `policyd` is the single authority for QoS/affinity policy. Tasks can self-modify within recipe limits.

**Affected Tasks**:

- ✅ TASK-0013 (QoS ABI)

- ✅ TASK-0042 (Affinity + QoS Budgets)

- ✅ TASK-0277 (SMP Parallelism Policy)

**Action**: None required (already aligned)

---

### 2. IPI Classes (Correctness vs. Best-effort)

**Status**: ✅ Documented in `docs/architecture/smp-ipi-rate-limiting.md`

**Decision**: Correctness IPIs (TLB shootdown) must not be dropped; best-effort IPIs (reschedule nudges) may be throttled.

**Affected Tasks**:

- ✅ TASK-0012 (SMP v1)

- ✅ TASK-0247 (RV virt SMP)

- ✅ TASK-0277 (SMP Parallelism Policy)

**Action**: None required (already aligned)

---

### 3. Rust Concurrency Model (Send/Sync, Ownership)

**Status**: ✅ Documented in `docs/architecture/16-rust-concurrency-model.md`

**Decision**: Per-CPU ownership for SMP v1; `unsafe impl Send/Sync` only when necessary with clear documentation.

**Affected Tasks**:

- ✅ TASK-0011B (Rust Idioms)

- ✅ TASK-0012 (SMP v1)

- ✅ TASK-0277 (SMP Parallelism Policy)

**Action**: None required (already aligned)

---

### 4. Newtype Construction Authority

**Status**: ✅ Documented in `docs/architecture/01-neuron-kernel.md`

**Decision**: Only specific kernel modules can construct newtypes (e.g., `TaskTable` for `Pid`, `CapTable` for `CapSlot`).

**Affected Tasks**:

- ✅ TASK-0011B (Rust Idioms)

- ✅ TASK-0267 (Kernel IPC)

**Action**: None required (already aligned)

---

### 5. Userland ABI Filters (NOT Kernel-Enforced)

**Status**: ✅ Documented in TASK-0019/0028 Security sections

**Decision**: TASK-0019/0028 are userland guardrails, NOT security boundaries. Kernel enforcement is TASK-0188.

**Affected Tasks**:

- ✅ TASK-0019 (Userland ABI Guardrails)

- ✅ TASK-0028 (ABI Filters v2)

- ✅ TASK-0188 (Kernel Sysfilter)

**Action**: None required (already aligned)

---

### 6. VMO Transfer Security (RO Sealing)

**Status**: ✅ Documented in TASK-0031 Security section

**Decision**: v1 uses library-level RO sealing convention; kernel-enforced sealing in v2+ (requires `Rights::SEAL` capability bit).

**Affected Tasks**:

- ✅ TASK-0031 (Zero-copy VMOs)

- ✅ TASK-0267 (Kernel IPC - capability transfer)

**Action**: None required (v1 decision documented; v2 kernel enforcement tracked separately)

---

### 7. Userspace Sandboxing (NOT Kernel-Enforced)

**Status**: ✅ Documented in TASK-0039 Security section

**Decision**: TASK-0039 is userspace confinement for compliant processes, NOT a security boundary against malicious code.

**Affected Tasks**:

- ✅ TASK-0039 (Sandboxing v1)

- ✅ TASK-0188 (Kernel Sysfilter - future enforcement)

**Action**: None required (already aligned)

---

### 8. Cooperative Memstat (NOT Kernel RSS)

**Status**: ✅ Documented in TASK-0228 Security section

**Decision**: TASK-0228 uses cooperative memstat (not kernel RSS). Future kernel RSS ABI is separate task.

**Affected Tasks**:

- ✅ TASK-0228 (OOM Watchdog)

**Action**: None required (already aligned)

---

### 9. Identity Binding Enforcement (DSoftBus)

**Status**: ⚠️ Needs verification in TASK-0004

**Decision**: `device_id` must be cryptographically bound to `noise_static_pub` before session acceptance.

**Affected Tasks**:

- ✅ TASK-0003B (Noise XK - handshake only)

- ⚠️ TASK-0004 (Identity binding enforcement)

- ⚠️ TASK-0005 (Cross-VM sessions)

**Action Required**: Verify TASK-0004 Security section explicitly covers identity binding enforcement.

---

### 10. Remote Gateway Authority (DSoftBus)

**Status**: ⚠️ Needs verification in TASK-0005

**Decision**: Remote gateway is deny-by-default; only `samgrd`/`bundlemgrd` proxied in v1.

**Affected Tasks**:

- ⚠️ TASK-0005 (Cross-VM sessions)

**Action Required**: Verify TASK-0005 Security section explicitly covers remote gateway authority model.

---

## Action Items Summary

### ✅ Completed (2026-01-09)

1. ✅ **TASK-0019**: Security section added (userland ABI filters NOT kernel-enforced)

2. ✅ **TASK-0028**: Security section added (learn→enforce, depends on TASK-0019)

3. ✅ **TASK-0031**: Security section added (VMO RO sealing = library convention in v1)

4. ✅ **TASK-0039**: Security section added (userspace sandboxing NOT kernel-enforced)

5. ✅ **TASK-0228**: Security section added (cooperative memstat limitations)
6. ✅ **docs/architecture/README.md**: Updated with inventory link

### ✅ All Verification Completed (2026-01-09)

**Verification Results**:

1. ✅ **TASK-0004**: Identity binding enforcement explicitly documented
   - Security Invariant: "`device_id` MUST be cryptographically bound to `noise_static_pub`"
   - Hardening Marker: "`dsoftbusd: identity mismatch peer=<id>`"

2. ✅ **TASK-0005**: Remote gateway authority model explicitly documented
   - Security Invariant: "Remote proxy MUST be deny-by-default: only `samgrd`, `bundlemgrd` proxied"
   - Attack Surface: "Remote gateway is a privileged proxy surface (highest risk)"

3. ✅ **TASK-0009**: TASK-0010 MMIO dependency explicitly documented
   - Constraints: "Kernel changes (required prerequisite): This is implemented as kernel work in **TASK-0010**"

**Cross-References Added**:

1. ✅ **TASK-0010**: Added "Unblocks: TRACK-DRIVERS-ACCELERATORS, TRACK-NETWORKING-DRIVERS"
2. ✅ **TASK-0031**: Added "Unblocks: TRACK-DRIVERS-ACCELERATORS (zero-copy DMA), TRACK-NETWORKING-DRIVERS"
3. ✅ **TASK-0012**: Added "Unblocks: TRACK-DRIVERS-ACCELERATORS (per-CPU driver scheduling)"
4. ✅ **TASK-0042**: Added "Unblocks: TRACK-DRIVERS-ACCELERATORS (QoS-aware driver scheduling)"

### Next Phase (Optimization)

1. Add kernel comparison analysis (seL4, Zircon, Linux) for each task category

2. Add Rust paradigm optimization suggestions

3. Create optimization roadmap for kernel tasks

---

## Maintenance Notes

- **Update Frequency**: This inventory should be updated whenever:

  - A new kernel-touch task is created
  - A task's security section is added/modified
  - A decision point is resolved or changed
  - SMP/QoS/parallelism architecture evolves

- **Ownership**: This document is maintained by the kernel team and reviewed during architecture sync meetings.

- **Related Documents**:

  - `docs/architecture/SECURITY-CONSISTENCY-CHECK.md` - Security drift prevention
  - `docs/architecture/smp-ipi-rate-limiting.md` - IPI policy
  - `docs/architecture/16-rust-concurrency-model.md` - Rust SMP patterns
  - `docs/architecture/01-neuron-kernel.md` - Kernel architecture

---

## Kernel Comparison & Optimization Analysis

### Purpose

This section provides systematic comparison of Open Nexus OS kernel tasks with established microkernels (seL4, Zircon) and monolithic kernels (Linux), along with Rust-specific optimization opportunities.

### Comparison Methodology

For each task category, we analyze:

1. **seL4 Approach** (C, formally verified, minimal TCB)

2. **Zircon Approach** (C++, Fuchsia, capability-based)

3. **Linux Approach** (C, monolithic, mature ecosystem)

4. **Rust Advantages** (memory safety, zero-cost abstractions, fearless concurrency)

5. **Optimization Opportunities** (specific to Open Nexus OS)

---

## Category 1: Core Kernel — Comparison & Optimization

### seL4 Comparison

**Architecture**:

- Formally verified microkernel (~10K LOC)

- Capability-based security (CSpace, VSpace)

- Fixed-priority preemptive scheduler

- IPC via synchronous endpoints

- No dynamic memory allocation in kernel

**Key Differences from Open Nexus**:

- seL4 uses formal verification (Isabelle/HOL proofs)

- seL4 has no SMP in verified kernel (experimental SMP exists)

- seL4 capabilities are unforgeable kernel objects (similar to our design)

**What We Can Learn**:

- Formal specification of capability invariants (even without full verification)

- Strict separation of policy (userspace) and mechanism (kernel)

- Bounded kernel execution paths (no unbounded loops)

### Zircon Comparison

**Architecture**:

- Microkernel for Fuchsia (~170K LOC)

- Capability-based (handles, rights)

- Multi-level feedback queue scheduler

- Asynchronous IPC (ports, channels)

- SMP with per-CPU runqueues

**Key Differences from Open Nexus**:

- Zircon uses C++ (vs our Rust)

- Zircon has more complex IPC (async ports vs our sync channels)

- Zircon scheduler is more sophisticated (MLFQ vs our simple round-robin + QoS)

**What We Can Learn**:

- Port-based async IPC patterns (for future v2)

- Job/Process hierarchy for resource management

- Deadline scheduling for real-time tasks

### Linux Comparison

**Architecture**:

- Monolithic kernel (~30M LOC)

- Discretionary Access Control (DAC) + LSM hooks

- Completely Fair Scheduler (CFS) with cgroups

- Synchronous + asynchronous syscalls

- Extensive SMP support (per-CPU, RCU, lockless data structures)

**Key Differences from Open Nexus**:

- Linux is monolithic (drivers in kernel vs our userspace drivers)

- Linux uses DAC/MAC (vs our capability-based model)

- Linux has mature SMP infrastructure (vs our v1 simple per-CPU)

**What We Can Learn**:

- RCU-like patterns for read-heavy data structures

- Per-CPU data structures to minimize cache coherence traffic

- Work stealing algorithms for load balancing

### Rust-Specific Optimization Opportunities (Core Kernel)

#### 1. Ownership-Based Capability Management (TASK-0011B, TASK-0267)

**Current State**: Capabilities stored in `CapTable` with runtime checks.

**Optimization**:

- Use Rust's type system to encode capability rights at compile time

- Phantom types for capability kinds (`Cap<T, Rights>`)

- Zero-cost capability rights checks (compile-time vs runtime)

**Example**:

```rust
// Current (runtime check)
fn map_vmo(cap: CapSlot) -> Result<(), Error> {
    let cap_kind = cap_table.get(cap)?;
    if !cap_kind.has_right(Rights::MAP) {
        return Err(Error::NoRights);
    }
    // ...
}

// Optimized (compile-time check)
fn map_vmo<R: HasMapRight>(cap: Cap<Vmo, R>) -> Result<(), Error> {
    // Rights checked at compile time, no runtime overhead
    // ...
}
```

**Impact**: Eliminates runtime capability rights checks in hot paths.

#### 2. Per-CPU Data with `!Send` Marker (TASK-0012, TASK-0277)

**Current State**: Per-CPU data accessed via CPU ID indexing.

**Optimization**:

- Use `!Send` marker to statically prevent cross-CPU data access

- `PerCpu<T>` wrapper that's `!Send` but `Sync` (read-only cross-CPU)

- Compiler-enforced per-CPU ownership

**Example**:

```rust
// Current (runtime CPU ID check)
fn schedule() {
    let cpu_id = current_cpu();
    let scheduler = &mut SCHEDULERS[cpu_id];
    scheduler.pick_next();
}

// Optimized (compile-time CPU affinity)
struct PerCpu<T: !Send>(T);

fn schedule(scheduler: &mut PerCpu<Scheduler>) {
    // Compiler guarantees this is CPU-local
    scheduler.0.pick_next();
}
```

**Impact**: Eliminates CPU ID checks, prevents accidental cross-CPU access.

#### 3. Zero-Copy IPC with Ownership Transfer (TASK-0267, TASK-0031)

**Current State**: IPC copies message bytes; VMO transfer via capability.

**Optimization**:

- Use Rust's move semantics for zero-copy message passing

- `IpcMessage<T>` takes ownership of payload

- Receiver gets owned payload (no copy)

**Example**:

```rust
// Current (copy-based)
fn send_message(chan: &Channel, data: &[u8]) {
    chan.send_bytes(data); // Copies data
}

// Optimized (move-based)
fn send_message<T>(chan: &Channel, msg: IpcMessage<T>) {
    chan.send(msg); // Moves ownership, no copy
}
```

**Impact**: Eliminates message copies for large payloads.

#### 4. Const Generics for Bounded Data Structures (TASK-0012, TASK-0188)

**Current State**: Runtime bounds checks on queues, tables.

**Optimization**:

- Use const generics for compile-time size bounds

- `BoundedVec<T, N>` with compile-time capacity

- Eliminates runtime capacity checks

**Example**:

```rust
// Current (runtime bounds)
struct Scheduler {
    queues: [VecDeque<Task>; 4],
}

// Optimized (compile-time bounds)
struct Scheduler<const MAX_TASKS: usize> {
    queues: [BoundedVec<Task, MAX_TASKS>; 4],
}
```

**Impact**: Eliminates runtime bounds checks, enables stack allocation.

#### 5. Newtype Pattern for Type-Safe Kernel Handles (TASK-0011B)

**Current State**: Partially implemented (Pid, CapSlot).

**Optimization**:

- Extend to all kernel handles (Asid, EndpointId, IrqId)

- Add `#[repr(transparent)]` for zero-cost abstraction

- Implement `From`/`Into` for safe conversions

**Status**: ✅ Already planned in TASK-0011B

**Impact**: Prevents handle type confusion bugs at compile time.

---

## Category 2: Bring-up/Boot — Comparison & Optimization

### seL4 Comparison (Bring-up/Boot)

**Boot Process**:

- Minimal boot code (ELF loader in kernel)

- Root task receives untyped memory capabilities

- No dynamic device discovery (static device tree)

**Key Differences**:

- seL4 has simpler boot (no ACPI/UEFI complexity)

- seL4 root task is privileged (can create capabilities)

**What We Can Learn**:

- Minimize kernel boot code (delegate to userspace)

- Static device configuration for determinism

### Zircon Comparison (Bring-up/Boot)

**Boot Process**:

- Bootloader (Gigaboot) loads ZBI (Zircon Boot Image)

- Kernel parses ZBI for device tree, ramdisk

- Userboot (first userspace process) starts component manager

**Key Differences**:

- Zircon has more complex boot (ZBI format, multiple stages)

- Zircon uses component framework (vs our simpler init)

**What We Can Learn**:

- Boot image format with integrity verification

- Structured boot data (vs ad-hoc parsing)

### Linux Comparison (Bring-up/Boot)

**Boot Process**:

- Bootloader (GRUB/U-Boot) loads kernel + initramfs

- Kernel decompresses, initializes subsystems

- Init system (systemd) starts services

**Key Differences**:

- Linux boot is complex (ACPI, PCI enumeration, module loading)

- Linux has extensive hardware support (vs our minimal QEMU virt)

**What We Can Learn**:

- Device tree parsing for hardware discovery

- Initramfs for early userspace

### Rust-Specific Optimization Opportunities (Bring-up/Boot)

#### 1. Const Evaluation for Boot-Time Configuration (TASK-0244, TASK-0245)

**Optimization**:

- Use `const fn` for device tree parsing at compile time

- Const-evaluated boot configuration (no runtime parsing)

**Example**:

```rust
// Compile-time device tree parsing
const UART_BASE: usize = const_parse_dt!("uart@10000000");
const UART_IRQ: u32 = const_parse_dt_irq!("uart@10000000");
```

**Impact**: Eliminates boot-time device tree parsing overhead.

#### 2. Type-State Pattern for Boot Stages (TASK-0245, TASK-0247)

**Optimization**:

- Use type-state pattern to enforce boot stage ordering

- `BootStage<Uninitialized>` → `BootStage<UartReady>` → `BootStage<Ready>`

- Compiler prevents out-of-order initialization

**Example**:

```rust
struct BootStage<S>(PhantomData<S>);
struct Uninitialized;
struct UartReady;
struct Ready;

impl BootStage<Uninitialized> {
    fn init_uart(self) -> BootStage<UartReady> { ... }
}

impl BootStage<UartReady> {
    fn init_plic(self) -> BootStage<Ready> { ... }
}
```

**Impact**: Prevents initialization ordering bugs at compile time.

---

## Category 3: Security/ABI — Comparison & Optimization

### seL4 Comparison (Security/ABI)

**Security Model**:

- Formally verified isolation (information flow proofs)

- Capability-based access control (no ambient authority)

- No syscall filtering (all operations capability-gated)

**Key Differences**:

- seL4 has formal proofs (vs our testing-based assurance)

- seL4 has no syscall filter (unnecessary with pure capability model)

**What We Can Learn**:

- Formal specification of security invariants

- Pure capability model (minimize syscall surface)

### Zircon Comparison (Security/ABI)

**Security Model**:

- Capability-based (handles with rights)

- Job/Process hierarchy for resource limits

- No syscall filtering (capability-gated operations)

**Key Differences**:

- Zircon uses handles (vs our CapSlots)

- Zircon has job-based resource limits (vs our per-task limits)

**What We Can Learn**:

- Handle rights as bitmask (efficient rights checks)

- Job hierarchy for resource management

### Linux Comparison (Security/ABI)

**Security Model**:

- DAC (user/group/other permissions)

- LSM (SELinux, AppArmor) for MAC

- Seccomp for syscall filtering

- Namespaces for isolation

**Key Differences**:

- Linux uses DAC/MAC (vs our capability-based model)

- Linux has mature syscall filtering (seccomp-bpf)

**What We Can Learn**:

- BPF-style syscall filters (efficient, programmable)

- Namespace-based isolation (vs our VFS namespaces)

### Rust-Specific Optimization Opportunities (Security/ABI)

#### 1. Type-Safe Syscall Dispatch (TASK-0188, TASK-0019)

**Optimization**:

- Use Rust enums for syscall dispatch (vs switch statement)

- Pattern matching for exhaustive syscall handling

- Compiler ensures all syscalls handled

**Example**:

```rust
enum Syscall {
    Send { chan: CapSlot, data: &[u8] },
    Recv { chan: CapSlot, buf: &mut [u8] },
    Map { vmo: CapSlot, addr: usize },
    // ...
}

fn dispatch(syscall: Syscall) -> Result<usize, Error> {
    match syscall {
        Syscall::Send { chan, data } => sys_send(chan, data),
        Syscall::Recv { chan, buf } => sys_recv(chan, buf),
        Syscall::Map { vmo, addr } => sys_map(vmo, addr),
    }
}
```

**Impact**: Eliminates invalid syscall numbers, exhaustive handling.

#### 2. Const-Evaluated Sysfilter Profiles (TASK-0188)

**Optimization**:

- Compile sysfilter profiles to const data structures

- Use const generics for profile size bounds

- Eliminates runtime profile parsing

**Example**:

```rust
const PROFILE: SysfilterProfile = const_parse_profile!(include_str!("profile.toml"));

fn check_syscall(syscall: Syscall) -> bool {
    PROFILE.allows(&syscall) // No runtime parsing
}
```

**Impact**: Eliminates profile parsing overhead, enables const evaluation.

---

## Optimization Roadmap

### Phase 1: Type Safety (Low-Hanging Fruit)

**Tasks**: TASK-0011B (Newtypes), TASK-0267 (IPC types)

**Optimizations**:

1. Extend newtype pattern to all kernel handles

2. Add phantom types for capability rights

3. Use type-state pattern for boot stages

**Expected Impact**: Compile-time bug prevention, zero runtime cost

### Phase 2: Per-CPU Optimization (SMP Foundation)

**Tasks**: TASK-0012 (SMP v1), TASK-0277 (SMP Policy)

**Optimizations**:

1. `!Send` marker for per-CPU data

2. Const generics for bounded queues

3. Lock-free per-CPU allocators

**Expected Impact**: Eliminates CPU ID checks, reduces cache coherence traffic

### Phase 3: Zero-Copy IPC (Performance Critical)

**Tasks**: TASK-0267 (Kernel IPC), TASK-0031 (VMOs)

**Optimizations**:

1. Move semantics for IPC messages

2. Compile-time capability rights checks

3. Zero-copy VMO transfer

**Expected Impact**: Eliminates message copies, reduces IPC latency

### Phase 4: Const Evaluation (Boot & Config)

**Tasks**: TASK-0244/245/247 (Bring-up), TASK-0188 (Sysfilter)

**Optimizations**:

1. Const device tree parsing

2. Const sysfilter profiles

3. Const boot configuration

**Expected Impact**: Eliminates runtime parsing, faster boot

---

## Recommendations for User Review

### High-Priority Optimizations (Implement Soon)

**Priority order (recommended)**:

1. **DriverKit ABI policy + core contracts first**: lock the stable boundary before implementation spreads.
2. **Type-safety improvements next** (newtypes, typed caps) because they are low-risk and reduce bug surface pre-SMP.
3. **Per-CPU `!Send` ownership wrapper** after the above (it ties directly into SMP implementation work).

Concrete tasks created for these:

- `docs/adr/0018-driverkit-abi-versioning-and-stability.md`
- `tasks/TASK-0280-driverkit-v1-core-contracts-queues-fences-buffers.md`
- `tasks/TASK-0281-kernel-newtypes-v1c-handle-typing.md`
- `tasks/TASK-0282-kernel-capability-phantom-rights-v1.md`
- `tasks/TASK-0283-kernel-percpu-ownership-wrapper-v1.md`
- `tasks/TASK-0284-userspace-dmabuffer-ownership-v1-prototype.md`

1. **Extend Newtype Pattern** (TASK-0011B)

   - Add `Asid`, `EndpointId`, `IrqId` newtypes
   - Prevents handle type confusion bugs

2. **Per-CPU `!Send` Marker** (TASK-0012)

   - Statically prevent cross-CPU data access
   - Eliminates CPU ID checks

3. **Phantom Types for Capability Rights** (TASK-0267)

   - Compile-time rights checks
   - Eliminates runtime overhead

### Medium-Priority Optimizations (Plan for v2)

1. **Zero-Copy IPC with Move Semantics** (TASK-0267, TASK-0031)

   - Requires IPC protocol redesign
   - Significant performance impact

2. **Const Generics for Bounded Structures** (TASK-0012, TASK-0188)

   - Eliminates runtime bounds checks
   - Enables stack allocation

3. **Type-State Pattern for Boot** (TASK-0245, TASK-0247)

   - Prevents initialization ordering bugs
   - Better than runtime assertions

### Low-Priority Optimizations (Future Work)

1. **Const Device Tree Parsing** (TASK-0244)

   - Requires const fn DTB parser
   - Marginal boot time improvement

2. **Const Sysfilter Profiles** (TASK-0188)

   - Requires const TOML parser
   - Marginal syscall dispatch improvement

---

---

## Userspace Driver Strategy: Deep Dive & Evaluation

### Vision: Minimal Kernel + Userspace Drivers

**Core Principle**: Keep kernel minimal by moving drivers to userspace, with kernel providing only:

1. **Safe MMIO/IRQ/DMA access** (capability-gated, bounded)
2. **Zero-copy buffer primitives** (VMOs with ownership transfer)
3. **Scheduling/QoS hints** (not driver logic)

**Driver Split**:

- **Kernel**: Thin capability broker (MMIO windows, IRQ routing, DMA isolation)
- **Userspace Driver Service**: Hardware commands, firmware protocol, command validation
- **SDK/DriverKit**: Shared abstractions (Queue, Fence, Buffer, Backpressure)

### Track Status & Linkage Analysis

#### ✅ TRACK-DRIVERS-ACCELERATORS.md

**Scope**: GPU, NPU, VPU, Audio, Camera, ISP, Storage, Sensors

**Key Dependencies**:

- ✅ TASK-0010 (Device MMIO Access) — **Linked**
- ✅ TASK-0031 (Zero-copy VMOs) — **Linked**
- ✅ TASK-0013 (QoS/Timers) — **Linked**
- ✅ TASK-0006 (Audit/Observability) — **Linked**

**Candidates**:

- CAND-DRV-000: DriverKit core (Queue/Submit/Fence/Waitset)
- CAND-DRV-010: GPU device-class service skeleton
- CAND-DRV-020: Audio device-class service
- CAND-DRV-030: Camera/ISP pipeline skeleton

**Status**: ✅ Well-linked, dependencies explicit

#### ✅ TRACK-NETWORKING-DRIVERS.md

**Scope**: virtio-net, nexus-net, DSoftBus integration

**Key Dependencies**:

- ✅ TASK-0010 (Device MMIO Access) — **Linked**
- ✅ TASK-0003/0004/0005 (Networking steps) — **Linked**

**Candidates**:

- CAND-NETDRV-001: virtio-net userspace frontend
- CAND-NETDRV-010: Zero-copy packet buffers

**Status**: ✅ Well-linked, dependencies explicit

#### ⚠️ Missing Cross-References

**In Kernel Tasks**:

- TASK-0010 (MMIO Access) mentions virtio but not TRACK-DRIVERS-ACCELERATORS
- TASK-0031 (VMOs) doesn't mention TRACK-DRIVERS-ACCELERATORS
- TASK-0012/0042 (SMP/QoS) don't mention driver scheduling implications

**Recommendation**: Add cross-references in "Unblocks" sections

---

### Rust Advantages for Userspace Drivers

#### 1. Memory Safety Without Performance Cost

**Problem (C drivers)**: Buffer overflows, use-after-free in DMA buffers

**Rust Solution**:

```rust
// Ownership prevents use-after-free
struct DmaBuffer {
    vmo: Vmo,
    mapping: VmoMapping,
}

impl DmaBuffer {
    fn submit_to_device(self, device: &Device) -> Fence {
        // Ownership transferred, can't use buffer until fence completes
        device.submit(self)
    }
}
```

**Impact**: Eliminates entire class of driver bugs at compile time

#### 2. Type-Safe Hardware Commands

**Problem (C drivers)**: Command buffer corruption, wrong command types

**Rust Solution**:

```rust
enum GpuCommand {
    Draw { vertices: u32, instances: u32 },
    Compute { workgroups: [u32; 3] },
    Blit { src: ImageHandle, dst: ImageHandle },
}

// Compiler ensures exhaustive handling
fn validate_command(cmd: &GpuCommand) -> Result<(), Error> {
    match cmd {
        GpuCommand::Draw { vertices, instances } => {
            if *vertices > MAX_VERTICES { return Err(Error::TooBig); }
            Ok(())
        }
        GpuCommand::Compute { workgroups } => { /* ... */ }
        GpuCommand::Blit { src, dst } => { /* ... */ }
    }
}
```

**Impact**: Prevents command type confusion, exhaustive validation

#### 3. Zero-Copy with Ownership Transfer

**Problem (C drivers)**: Manual refcounting, double-free, memory leaks

**Rust Solution**:

```rust
// Producer owns buffer
let buffer = DmaBuffer::allocate(size)?;
buffer.write_data(&data);

// Transfer ownership to device (no copy)
let fence = device.submit(buffer);

// Consumer gets owned buffer after fence
let result = fence.wait()?;
// result: DmaBuffer (owned, can't be used by device anymore)
```

**Impact**: Eliminates refcounting bugs, guarantees single owner

#### 4. Fearless Concurrency for Multi-Queue Devices

**Problem (C drivers)**: Data races in multi-queue submission

**Rust Solution**:

```rust
// Each queue is Send but not Sync (single-threaded access)
struct DeviceQueue {
    ring: VecDeque<Command>,
}

// Device manages multiple queues safely
struct Device {
    queues: Vec<Mutex<DeviceQueue>>, // Explicit locking
}

// Compiler prevents data races
fn submit_parallel(device: &Device, cmds: Vec<Command>) {
    cmds.into_par_iter().for_each(|cmd| {
        let queue_id = cmd.queue_hint();
        let mut queue = device.queues[queue_id].lock();
        queue.push(cmd); // Safe: lock held
    });
}
```

**Impact**: Prevents data races in multi-queue devices at compile time

#### 5. Const Generics for Device Limits

**Problem (C drivers)**: Runtime bounds checks, buffer overflows

**Rust Solution**:

```rust
// Device limits encoded in type
struct GpuDevice<const MAX_TEXTURES: usize, const MAX_BUFFERS: usize> {
    textures: BoundedVec<Texture, MAX_TEXTURES>,
    buffers: BoundedVec<Buffer, MAX_BUFFERS>,
}

// Compiler enforces limits
impl<const MAX_TEXTURES: usize, const MAX_BUFFERS: usize> 
    GpuDevice<MAX_TEXTURES, MAX_BUFFERS> 
{
    fn bind_texture(&mut self, tex: Texture) -> Result<(), Error> {
        self.textures.try_push(tex) // Compile-time capacity check
    }
}
```

**Impact**: Eliminates runtime bounds checks, stack allocation

---

### Kernel Optimization Benefits

#### 1. Reduced Kernel LOC (Complexity Reduction)

**Comparison**:

| Kernel | Driver LOC | Kernel LOC | Ratio |
| -------- | ----------- | ----------- | ------- |
| **Linux** | ~5M (in-kernel) | ~30M total | 17% drivers |
| **Zircon** | ~50K (userspace) | ~170K total | 29% drivers |
| **seL4** | 0 (userspace) | ~10K total | 0% drivers |
| **Open Nexus** | 0 (userspace) | ~15K target | 0% drivers |

**Impact**: Smaller kernel TCB, easier verification, faster boot

#### 2. Crash Isolation

**Problem (Monolithic)**: Driver crash = kernel panic

**Solution (Userspace)**:

- Driver service crashes → kernel unaffected
- Device reset → service restart
- Audit trail preserved

**Rust Advantage**: `panic = abort` in driver service (no unwinding overhead)

#### 3. Zero-Copy DMA with Ownership

**Problem (Linux)**: DMA buffers require manual refcounting (`get_page`/`put_page`)

**Solution (Open Nexus)**:

```rust
// Kernel provides VMO capability
let vmo = sys_vmo_create(size, Rights::MAP | Rights::DMA)?;

// Driver maps for CPU access
let mapping = vmo.map_rw()?;
mapping.write_data(&data);

// Transfer to device (ownership moved)
let fence = device.submit_dma(vmo); // vmo consumed

// Kernel tracks ownership, prevents double-use
```

**Impact**: Eliminates refcounting bugs, kernel doesn't track DMA buffer state

#### 4. Per-Device QoS Scheduling

**Problem (Linux)**: Device scheduling mixed with CPU scheduling

**Solution (Open Nexus)**:

- Kernel provides QoS hints (Idle/Normal/Interactive/PerfBurst)
- Driver service implements device-specific scheduling
- Kernel doesn't need device-specific logic

**Rust Advantage**: Type-safe QoS enum, exhaustive matching

#### 5. IOMMU/GPU-MMU Isolation (Future)

**Problem**: Malicious driver can DMA to arbitrary memory

**Solution**:

- Kernel manages IOMMU page tables
- Driver gets restricted DMA window (capability)
- Rust prevents driver from forging DMA addresses

**Status**: Future work (requires hardware support)

---

### Metal-like SDK Strategy: Evaluation

#### Concept

**Metal Model** (Apple):

- Thin driver (command submission, synchronization)
- Rich SDK (shader compiler, pipeline state, resource management)
- Zero-copy buffers (MTLBuffer with CPU/GPU coherence)

**Open Nexus Adaptation**:

- **NexusGfx SDK** (Metal-like API)
- **GPU Driver Service** (command validation, submission)
- **Kernel** (MMIO access, VMO/DMA, fences)

#### Advantages

1. **Smaller Driver Code**

   - Driver: ~5K LOC (command submission, reset)
   - SDK: ~50K LOC (API, validation, optimization)
   - Kernel: ~500 LOC (MMIO broker, fence primitives)

   **Total**: ~55K LOC vs Linux GPU driver ~200K LOC

2. **Better Security**

   - SDK runs in app process (sandboxed)
   - Driver validates commands (deny-by-default)
   - Kernel enforces MMIO bounds (W^X)

   **Rust Advantage**: SDK can use `unsafe` for performance, driver stays safe

3. **Faster Development**

   - SDK updates don't require driver changes
   - Driver is stable ABI (versioned)
   - Apps link SDK directly (no syscall overhead)

4. **Zero-Copy Pipeline**

   ```rust
   // App allocates buffer
   let buffer = gfx.create_buffer(size)?;
   
   // App writes data (CPU-side)
   buffer.write(&vertices);
   
   // Submit to GPU (ownership transfer, no copy)
   let fence = gfx.draw(buffer, pipeline);
   
   // Wait for completion
   fence.wait()?;
   ```

   **Impact**: Eliminates kernel→driver→GPU copies

#### Challenges

1. **Command Validation Overhead**

   **Problem**: Driver must validate every command (untrusted SDK)

   **Rust Solution**: Zero-cost validation with type-safe commands

   ```rust
   // Validation is pattern matching (zero-cost)
   fn validate(cmd: &GpuCommand) -> Result<(), Error> {
       match cmd {
           GpuCommand::Draw { vertices, .. } if *vertices <= MAX => Ok(()),
           _ => Err(Error::Invalid),
       }
   }
   ```

2. **Synchronization Complexity**

   **Problem**: CPU/GPU synchronization requires fences

   **Rust Solution**: Ownership-based fences

   ```rust
   // Fence owns buffer until GPU completes
   struct Fence {
       buffer: DmaBuffer, // Owned
       device_handle: DeviceHandle,
   }
   
   impl Fence {
       fn wait(self) -> Result<DmaBuffer, Error> {
           // Poll device
           while !self.device_handle.is_complete() { yield; }
           Ok(self.buffer) // Return ownership
       }
   }
   ```

3. **Vendor Blob Integration**

   **Problem**: Some devices require proprietary firmware

   **Solution**: Sandbox vendor blob in driver service

   - Blob runs in driver process (isolated from kernel)
   - Kernel enforces MMIO bounds (blob can't escape)
   - Rust driver validates blob commands

   **Status**: Requires IOMMU for full isolation (future)

---

### Comparison with Other Kernels

#### seL4 (Formally Verified)

**Driver Model**: All drivers in userspace

**Advantages**:

- Minimal kernel TCB (~10K LOC)
- Formal verification possible

**Disadvantages**:

- No SMP in verified kernel
- Manual capability management (error-prone)

**Open Nexus Improvement**: Rust ownership eliminates manual capability management

#### Zircon (Fuchsia)

**Driver Model**: Userspace drivers with DriverKit SDK

**Advantages**:

- Crash isolation
- Rich SDK (FIDL, async I/O)

**Disadvantages**:

- C++ (manual memory management)
- Complex async model (ports, channels)

**Open Nexus Improvement**: Rust eliminates memory bugs, simpler sync model

#### Linux (Monolithic)

**Driver Model**: Drivers in kernel

**Advantages**:

- Mature ecosystem
- High performance (no context switches)

**Disadvantages**:

- Driver bugs crash kernel
- Huge TCB (~30M LOC)
- Manual refcounting (DMA buffers)

**Open Nexus Improvement**: Userspace isolation + Rust safety + zero-copy

---

### Optimization Recommendations

#### High-Priority (Implement Now)

1. **Add Cross-References in Kernel Tasks**

   - TASK-0010: Add "Unblocks: TRACK-DRIVERS-ACCELERATORS, TRACK-NETWORKING-DRIVERS"
   - TASK-0031: Add "Unblocks: TRACK-DRIVERS-ACCELERATORS (zero-copy DMA)"
   - TASK-0012/0042: Add "Unblocks: TRACK-DRIVERS-ACCELERATORS (driver QoS scheduling)"

2. **Document DriverKit ABI Stability**

   - Create ADR for DriverKit versioning
   - Define stable ABI surface (Queue, Fence, Buffer)
   - Document breaking change policy

3. **Prototype Ownership-Based DMA**

   - Implement `DmaBuffer` with ownership transfer
   - Prove zero-copy in host tests
   - Add QEMU markers for DMA submission

#### Medium-Priority (Plan for v2)

1. **IOMMU Integration**

   - Design kernel IOMMU page table management
   - Define restricted DMA window capability
   - Prove isolation in negative tests

2. **Multi-Queue Device Support**

   - Design per-queue ownership model
   - Implement work stealing for load balancing
   - Add QoS-aware queue selection

3. **Vendor Blob Sandboxing**

   - Define blob loading policy (signed, audited)
   - Implement blob→driver IPC boundary
   - Add runtime validation hooks

#### Low-Priority (Future Work)

1. **GPU Shader Compilation**

   - Integrate SPIR-V compiler (host-first)
   - Define shader validation policy
   - Add shader cache for performance

2. **Real-Time Scheduling**

   - Add deadline scheduling for audio/video
   - Implement priority inheritance for fences
   - Prove latency bounds in tests

---

### Conclusion: Is the Strategy Optimal?

#### ✅ Strengths

1. **Rust Ownership = Zero-Copy DMA**: Eliminates refcounting bugs, kernel doesn't track buffer state
2. **Minimal Kernel TCB**: Drivers in userspace → smaller kernel (~15K LOC target)
3. **Crash Isolation**: Driver crash doesn't affect kernel
4. **Type-Safe Commands**: Prevents command corruption at compile time
5. **Metal-like SDK**: Smaller driver code (~5K LOC vs ~200K LOC in Linux)

#### ⚠️ Challenges

1. **Command Validation Overhead**: Every command must be validated (but Rust makes this zero-cost)
2. **Synchronization Complexity**: CPU/GPU fences require careful design (but ownership helps)
3. **Vendor Blobs**: Proprietary firmware requires sandboxing (future IOMMU work)

#### 🎯 Verdict

**Optimal for Open Nexus Goals**:

- ✅ Security: Userspace isolation + Rust safety
- ✅ Complexity: Smaller kernel TCB
- ✅ Performance: Zero-copy DMA with ownership
- ✅ Maintainability: Smaller driver code (~5K LOC)

**Recommendation**: Proceed with userspace driver strategy, prioritize:

1. Cross-reference kernel tasks with TRACK files
2. Document DriverKit ABI stability
3. Prototype ownership-based DMA

---

## Final Assessment (2026-01-09): Are Kernel Tasks “Best-Optimized” Now?

### What is now best-in-class (for this project stage)

- **Task-level security coverage**: kernel-touch tasks now consistently carry threat models, invariants, and “DON'T DO” guidance.
- **Anti-drift decisions are explicit**: QoS/Affinity authority and IPI correctness vs best-effort are clearly documented and cross-linked.
- **Driver strategy is coherent**: the repo has a clear “thin kernel, safe userspace driver” path, plus a Metal-like SDK direction.
- **Tracks are wired into kernel prerequisites**: MMIO access + VMO plumbing + SMP/QoS tasks now explicitly reference the driver/accelerator tracks.

### What is still “not guaranteed optimal” (and why)

Even with strong documentation, **optimality depends on implementation choices** that are now captured as tasks/ADR:

- **DriverKit ABI stability**: without an ADR, DriverKit tends to fragment. This is now fixed by `ADR-0018` and extracted tasks (TASK-0280).
- **DMA isolation**: zero-copy without IOMMU/GPU-MMU still needs a trust model (vendor blobs vs trusted services). This remains a future hardware/kernel capability.
- **SMP correctness**: per-CPU ownership patterns are “best practice” for Rust kernels, but we still must implement them carefully (TASK-0283).

### Verdict

**For the current planning/documentation layer, the kernel task inventory is now as optimized as it can reasonably be**:

- minimal-kernel direction is consistent with seL4/Zircon principles,
- Rust paradigms (ownership/type safety) are explicitly leveraged,
- and the remaining “optimality” work is expressed as concrete tasks and an ADR, instead of being implicit.

---

## Next Steps

1. **User Review**: Review optimization recommendations and prioritize

2. **Add Cross-References**: Link TASK-0010/0031/0012/0042 with TRACK files

3. **Create Optimization Tasks**: Break down high-priority optimizations into concrete tasks

4. **Document DriverKit ABI**: Create ADR for versioning and stability

5. **Verify Remaining Action Items**: Complete TASK-0004/0005/0009 verification
