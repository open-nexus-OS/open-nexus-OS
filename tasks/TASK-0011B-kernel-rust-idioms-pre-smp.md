---
title: TASK-0011B Kernel Rust idioms & ownership clarity (pre-SMP prep)
status: Draft
owner: @kernel-team
created: 2026-01-09
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Parent: tasks/TASK-0011-kernel-simplification-phase-a.md
  # NOTE: RFC-0001 is the seed contract for TASK-0011 (layout + headers). TASK-0011B requires its own seed RFC.
  - Seed RFC: docs/rfcs/RFC-0020-kernel-ownership-and-rust-idioms-pre-smp-v1.md
  - Background (parent contract): docs/rfcs/RFC-0001-kernel-simplification.md
  - Kernel overview: docs/architecture/01-neuron-kernel.md
  - SMP follow-up: tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md
---

## Context

TASK-0011 (Phase A) adds documentation headers but does not optimize for Rust-specific paradigms.
Before SMP work (TASK-0012), we should leverage Rust's strengths:

1. **Ownership clarity**: Make data ownership explicit (who owns Scheduler? TaskTable? IPC Router?)
2. **Fearless concurrency prep**: Prepare structures for SMP by making `Send`/`Sync` boundaries explicit
3. **Type-driven safety**: Use newtypes and type states to prevent misuse at compile time
4. **Error propagation**: Unify error handling patterns (currently mixed `-errno` and `Result<T, E>`)

This is inspired by **Servo's parallel architecture** (parallel layout, parallel styling) where Rust's
ownership model enables safe parallelism without locks.

## Goal

Refactor kernel structures to be **Rust-idiomatic** and **SMP-ready**, with zero behavior change
(verified by existing QEMU marker contract).

## Non-Goals

- Any SMP implementation (that's TASK-0012)
- Subcrate split (that's Phase C, later)
- Syscall API changes (userspace-visible ABI stays stable)

## Constraints / invariants (hard requirements)

- **Logic-preserving only**: no runtime behavior changes, markers stay identical
- **Determinism**: QEMU marker contract stays green
- **ABI stability**: syscall numbers and error codes unchanged
- **Performance**: no measurable regression (boot time, syscall latency)

## Red flags / decision points

- **RED**:
  - None. If a change risks behavior, it's out of scope.
- **YELLOW**:
  - Ownership changes can be subtle; need careful review of `unsafe` blocks.
  - Type changes may require updating many call sites (keep mechanical).
- **GREEN**:
  - Rust's borrow checker will catch most ownership mistakes at compile time.
  - This is the ideal time (after docs, before SMP behavioral changes).

## Security considerations

### Threat model

- Data races during future SMP implementation (mitigated by making `Send`/`Sync` explicit now)
- Use-after-free in kernel structures (mitigated by ownership clarification)
- Type confusion in capability handling (mitigated by newtypes)

### Security invariants (MUST hold)

All existing security invariants from TASK-0011 remain unchanged, plus:

- **Ownership invariants**: Each kernel object has exactly one owner (no shared mutable state without synchronization)
- **Send/Sync boundaries**: Only explicitly `Send`/`Sync` types can cross thread boundaries (prep for SMP)
- **Capability type safety**: Capability kinds are statically typed where possible (reduce runtime checks)
- **Error propagation**: Security-critical errors cannot be silently ignored (use `#[must_use]`)

### DON'T DO (explicit prohibitions)

- DON'T introduce `unsafe` without explicit justification and safety comments
- DON'T use `RefCell` in hot paths (runtime borrow checking overhead)
- DON'T make structures `Send`/`Sync` without verifying safety invariants
- DON'T change error semantics (e.g., turning panics into `Result` or vice versa)
- DON'T add `#[allow(dead_code)]` to hide ownership issues
- DON'T use `transmute` or pointer casts to bypass type safety

### Attack surface impact

- Minimal: Refactoring reduces attack surface by making invariants explicit
- Type-driven safety prevents entire classes of bugs (type confusion, use-after-free)

### Mitigations

- Rust's borrow checker enforces ownership at compile time
- `Send`/`Sync` markers prevent accidental data races
- Newtypes prevent capability handle confusion
- `#[must_use]` on error types prevents ignored security failures

## Contract sources (single source of truth)

- Seed RFC: `docs/rfcs/RFC-0020-kernel-ownership-and-rust-idioms-pre-smp-v1.md`
- Background: `docs/rfcs/RFC-0001-kernel-simplification.md` (layout/taxonomy from TASK-0011)
- `docs/architecture/01-neuron-kernel.md`
- `scripts/qemu-test.sh` marker contract (must not change)

## Stop conditions (Definition of Done)

- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` passes with **no marker changes**
- `cargo test --workspace` passes (all unit tests green)
- `just diag-os` passes (kernel compiles for RISC-V)
- No new `unsafe` blocks without safety comments
- `#[must_use]` is applied per RFC-0020 Phase 3 contract (kernel-internal error envelope + hard-to-ignore errors; no syscall ABI/errno semantic changes)
- Seed RFC exists and is linked from this task (contract/decision seed; execution/proofs remain in this task)

## Touched paths (allowlist)

- `source/kernel/neuron/src/**` (implementation changes allowed)
- `docs/architecture/01-neuron-kernel.md` (document ownership model)
- `docs/rfcs/RFC-0020-kernel-ownership-and-rust-idioms-pre-smp-v1.md` (seed contract for this task)

## Plan (small PRs)

### 1. Ownership documentation (docs-first)

**Goal**: Document the current ownership model explicitly.

**Changes**:

- Add "Ownership model" section to `docs/architecture/01-neuron-kernel.md`:

  ```markdown
  ## Ownership Model (Rust-specific)
  
  ### Global Kernel State
  - `KERNEL_STATE: MaybeUninit<KernelState>` (static mut, initialized once in `kmain`)
  - Owner: Kernel (single-threaded until SMP)
  
  ### KernelState fields
  - `hal: VirtMachine` — Owned by KernelState, never shared
  - `scheduler: Scheduler` — Owns all TaskTable entries
  - `tasks: TaskTable` — Owns Task structs, borrows to Scheduler
  - `ipc: Router` — Owns message queues, borrows to syscall handlers
  - `address_spaces: AddressSpaceManager` — Owns page tables, borrows to mm syscalls
  
  ### Capability ownership
  - Each Task owns its CapabilityTable (32 slots)
  - Capabilities are Copy-on-transfer (rights intersection)
  - No shared ownership (no Rc/Arc in kernel)
  
  ### Lifetimes
  - Most kernel structures have 'static lifetime (never deallocated)
  - Temporary borrows in syscall handlers (e.g., &mut Scheduler)
  - No heap-allocated lifetimes (all from global HEAP)
  ```

**Proof**: Documentation review (no code changes).

---

### 2. Newtype wrappers for kernel handles

**Goal**: Prevent handle confusion (ASID vs. TaskId vs. CapSlot).

**Current problem**:

```rust
// All are u32, easy to mix up!
pub type AsHandle = u32;
pub type Pid = u32;
pub type CapSlot = u32;
```

**Proposed**:

```rust
// source/kernel/neuron/src/types.rs (new file or extend existing)

/// Address Space Handle (opaque ASID wrapper)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct AsHandle(u32);

impl AsHandle {
    /// Create from raw ASID (kernel-internal only)
    pub(crate) const fn from_raw(asid: u32) -> Self {
        Self(asid)
    }

    /// Extract raw ASID (for hardware SATP writes)
    pub(crate) const fn as_raw(self) -> u32 {
        self.0
    }
}

/// Process ID (opaque task identifier)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Pid(u32);

impl Pid {
    pub(crate) const fn from_raw(id: u32) -> Self {
        Self(id)
    }

    pub(crate) const fn as_raw(self) -> u32 {
        self.0
    }
}

/// Capability Slot (index into task-local capability table)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct CapSlot(u32);

impl CapSlot {
    pub(crate) const fn from_raw(slot: u32) -> Self {
        Self(slot)
    }

    pub(crate) const fn as_raw(self) -> u32 {
        self.0
    }

    /// Bootstrap endpoint slot (always 0)
    pub const BOOTSTRAP: Self = Self(0);
}
```

**Benefits**:

- Compile-time prevention of `scheduler.activate(cap_slot)` (type error!)
- Self-documenting code (`AsHandle` vs. `u32`)
- Prep for SMP (can add `Send`/`Sync` bounds selectively)

**Migration**:

- Replace all `pub type AsHandle = u32` with newtype
- Update call sites (mostly mechanical: `AsHandle::from_raw(...)`)
- Verify with `cargo check`

**Proof**:

- `cargo test --workspace` (unit tests)
- `just diag-os` (kernel compiles)
- QEMU markers unchanged

---

### 3. Explicit Send/Sync markers (SMP prep)

**Goal**: Make concurrency boundaries explicit before SMP.

**Current problem**:

- Most kernel types are implicitly `!Send` and `!Sync` (contain raw pointers or `*mut`)
- SMP will require explicit `Send`/`Sync` for per-CPU structures

**Proposed**:

```rust
// source/kernel/neuron/src/sched/mod.rs

/// Scheduler state (single-threaded until SMP)
pub struct Scheduler {
    queues: [VecDeque<Pid>; 4], // QoS buckets
    current: Option<Pid>,
}

// Explicitly NOT Send/Sync (will be per-CPU in SMP)
// (This is a no-op now, but documents intent)
impl !Send for Scheduler {}
impl !Sync for Scheduler {}

// Alternative: Use PhantomData to make it explicit
use core::marker::PhantomData;

pub struct Scheduler {
    queues: [VecDeque<Pid>; 4],
    current: Option<Pid>,
    _not_send_sync: PhantomData<*const ()>, // Explicitly !Send + !Sync
}
```

**For SMP-safe structures**:

```rust
// source/kernel/neuron/src/hal/virt.rs

/// HAL machine state (immutable after init, safe to share)
pub struct VirtMachine {
    uart_base: usize,
    timer_freq: u64,
}

// Explicitly Send + Sync (immutable, safe for SMP)
unsafe impl Send for VirtMachine {}
unsafe impl Sync for VirtMachine {}

// Safety: VirtMachine is immutable after initialization.
// All fields are plain integers (no raw pointers).
```

**Benefits**:

- Documents concurrency intent
- Prevents accidental `Arc<Scheduler>` (would be compile error)
- Makes SMP refactor easier (know what needs per-CPU cloning)

**Proof**: Compile-time only (no runtime behavior change).

---

### 4. Error type unification (Result-based)

**Goal**: Unify error handling (currently mixed `-errno` and `Result<T, E>`).

**Current problem**:

```rust
// Syscall handlers return isize (POSIX-style)
pub fn sys_spawn(...) -> isize {
    // ...
    return -libc::EINVAL as isize; // Manual error encoding
}

// But internal functions use Result
fn validate_entry(pc: usize) -> Result<(), SpawnError> {
    if pc < TEXT_START {
        return Err(SpawnError::InvalidEntryPoint);
    }
    Ok(())
}
```

**Proposed**:

```rust
// source/kernel/neuron/src/syscall/mod.rs

/// Syscall result (internal representation)
pub type SyscallResult<T> = Result<T, SyscallError>;

/// Unified syscall error (convertible to -errno)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use] // Security: errors must be handled
pub enum SyscallError {
    /// Permission denied (capability check failed)
    PermissionDenied,
    /// Invalid argument (malformed input)
    InvalidArgument,
    /// No such process/capability
    NotFound,
    /// Out of memory (heap/ASID exhausted)
    OutOfMemory,
    /// Not implemented (disabled feature)
    NotImplemented,
}

impl SyscallError {
    /// Convert to POSIX errno (for userspace ABI)
    pub const fn to_errno(self) -> isize {
        match self {
            Self::PermissionDenied => -libc::EPERM as isize,
            Self::InvalidArgument => -libc::EINVAL as isize,
            Self::NotFound => -libc::ENOENT as isize,
            Self::OutOfMemory => -libc::ENOMEM as isize,
            Self::NotImplemented => -libc::ENOSYS as isize,
        }
    }
}

// Syscall handlers now return Result
pub fn sys_spawn(...) -> SyscallResult<Pid> {
    let entry_pc = validate_entry(args.pc)?; // ? operator!
    let task = scheduler.spawn(entry_pc)?;
    Ok(task.pid)
}

// Wrapper converts Result to isize for ABI
pub fn syscall_dispatch(num: usize, args: Args) -> isize {
    match num {
        SYSCALL_SPAWN => sys_spawn(args)
            .map(|pid| pid.as_raw() as isize)
            .unwrap_or_else(|e| e.to_errno()),
        // ...
    }
}
```

**Benefits**:

- `?` operator for error propagation (less boilerplate)
- `#[must_use]` prevents ignored errors (security!)
- Clear error semantics (no magic `-1` values)
- Easier to add error context (can wrap in `Result<T, (SyscallError, &'static str)>`)

**Migration**:

- Add `SyscallError` enum
- Convert syscall handlers one-by-one
- Keep ABI unchanged (`to_errno()` preserves `-errno` values)

**Proof**:

- `cargo test --workspace` (unit tests)
- QEMU markers unchanged (error handling behavior identical)

---

### 5. Capability type safety (phantom types)

**Goal**: Statically distinguish capability kinds at compile time.

**Current problem**:

```rust
pub enum CapabilityKind {
    Endpoint,
    Vmo,
    AddressSpace,
    Mmio,
}

pub struct Capability {
    kind: CapabilityKind,
    base: usize,
    len: usize,
    rights: Rights,
}

// Runtime check required!
fn send_to_endpoint(cap: Capability) -> Result<(), IpcError> {
    if cap.kind != CapabilityKind::Endpoint {
        return Err(IpcError::WrongCapabilityType);
    }
    // ...
}
```

**Proposed** (phantom type approach):

```rust
// source/kernel/neuron/src/cap/mod.rs

use core::marker::PhantomData;

/// Capability type marker (zero-sized)
pub trait CapabilityType {
    const KIND: CapabilityKind;
}

pub struct Endpoint;
impl CapabilityType for Endpoint {
    const KIND: CapabilityKind = CapabilityKind::Endpoint;
}

pub struct Vmo;
impl CapabilityType for Vmo {
    const KIND: CapabilityKind = CapabilityKind::Vmo;
}

/// Typed capability (zero-cost abstraction)
#[repr(transparent)]
pub struct Capability<T: CapabilityType> {
    inner: UntypedCapability,
    _marker: PhantomData<T>,
}

struct UntypedCapability {
    kind: CapabilityKind,
    base: usize,
    len: usize,
    rights: Rights,
}

impl<T: CapabilityType> Capability<T> {
    /// Downcast from untyped (runtime check once)
    pub fn from_untyped(cap: UntypedCapability) -> Result<Self, CapError> {
        if cap.kind != T::KIND {
            return Err(CapError::WrongType);
        }
        Ok(Self {
            inner: cap,
            _marker: PhantomData,
        })
    }
    
    /// Upcast to untyped (always safe)
    pub fn into_untyped(self) -> UntypedCapability {
        self.inner
    }
}

// Now type-safe!
fn send_to_endpoint(cap: Capability<Endpoint>) -> Result<(), IpcError> {
    // No runtime check needed, type system guarantees it's an Endpoint
    // ...
}
```

**Benefits**:

- Compile-time type safety (can't pass VMO to `send_to_endpoint`)
- Zero runtime cost (`#[repr(transparent)]`)
- Self-documenting APIs (`Capability<Endpoint>` vs. `Capability`)

**Trade-offs**:

- More complex type signatures
- Requires runtime check at capability table lookup (once per syscall)

**Decision**: Optional optimization, only if type confusion is a real risk.

**Proof**: Compile-time + unit tests.

---

### 6. Ownership transfer for IPC (move semantics)

**Goal**: Use Rust's move semantics for capability transfer.

**Current problem**:

```rust
// Capability transfer is Copy-based (implicit clone)
pub fn cap_transfer(src_slot: CapSlot, dst_task: Pid, dst_slot: CapSlot) -> Result<(), CapError> {
    let cap = self.get_cap(src_slot)?; // Copy
    let derived = cap.derive(rights_mask)?; // Copy
    dst_task.insert_cap(dst_slot, derived)?; // Copy
    Ok(())
}
```

**Proposed** (move semantics):

```rust
pub enum CapTransferMode {
    /// Copy capability (original remains valid)
    Copy,
    /// Move capability (original is revoked)
    Move,
}

pub fn cap_transfer(
    src_slot: CapSlot,
    dst_task: Pid,
    dst_slot: CapSlot,
    mode: CapTransferMode,
) -> Result<(), CapError> {
    let cap = match mode {
        CapTransferMode::Copy => self.get_cap(src_slot)?.clone(),
        CapTransferMode::Move => self.take_cap(src_slot)?, // Removes from src
    };
    
    let derived = cap.derive(rights_mask)?;
    dst_task.insert_cap(dst_slot, derived)?;
    Ok(())
}
```

**Benefits**:

- Explicit ownership transfer (clearer semantics)
- Prep for revocation (moved caps can be tracked)
- Matches Rust's ownership model

**Trade-offs**:

- ABI change (need new syscall or flag)
- More complex than current Copy-based approach

**Decision**: Optional, only if revocation is planned.

---

## Acceptance criteria (behavioral)

- All QEMU markers unchanged (deterministic boot)
- No new `unsafe` without safety comments
- Ownership model documented in architecture doc
- `Send`/`Sync` boundaries explicit (via `PhantomData` or negative impls)
- Kernel error envelope + `#[must_use]` applied (RFC-0020 Phase 3), with syscall ABI/errno semantics unchanged
- Capability type-safety wrappers applied minimally (RFC-0020 Phase 4), with runtime checks and ABI unchanged
- IPC/cap transfer semantics refactored as internal prep only (RFC-0020 Phase 5), with syscall behavior unchanged

## Evidence (to paste into PR)

- QEMU: `./scripts/qemu-test.sh` output (markers identical to baseline)
- Tests: `cargo test --workspace` summary (all green)
- Compile: `just diag-os` (no warnings)
- Diff: Show newtype conversions are mechanical (no logic changes)

## RFC seeds (for later)

After this task completes, consider follow-up RFCs as needed (not required for TASK-0011B).

---

## Priority ranking (if time-constrained)

**Execution order** (still one task; all sections 1–6 are in scope):

1. Ownership documentation (Section 1)
2. Newtype wrappers (Section 2) — Prevents handle confusion in SMP
3. Send/Sync markers (Section 3) — Forces explicit scheduler/thread-boundary decisions pre-SMP
4. Error envelope + `#[must_use]` (Section 4) — Makes failure paths hard to ignore without ABI change
5. Capability type-safety (Section 5) — Reduces “wrong cap kind” bugs before concurrency
6. IPC/cap transfer semantics (Section 6) — **internal prep only**; no syscall ABI/behavior changes in this task
