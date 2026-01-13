<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Rust Standards (Kernel + OS + Userspace)

**Status**: Active  
**Owners**: @kernel-team, @runtime  
**Created**: 2026-01-09  
**Purpose**: Define Rust best practices and layering rules so the codebase stays correct, auditable, and easy to evolve.

---

## Why this exists

Open Nexus OS is built to be **host-first, QEMU-last** and to stay **change-friendly**: foundations must be strict enough
to prevent drift, but flexible enough that adding features later does not require “fixes underneath”.

This document codifies:

- **Where `std` is preferred** and where `no_std` is required.
- **Lint + warning policy**, especially for the kernel.
- **Unsafe policy** and how to write auditable low-level Rust.
- A **layered code philosophy** (kernel vs libraries vs services vs tests).

---

## 1) `std` vs `no_std` (best practice)

### Rule 1.1 — Host-first domain code prefers `std`

- **Userspace domain libraries** in `userspace/` are expected to be **host-testable** and therefore typically use `std`.
- Benefits: better tooling, richer tests (property tests, Miri where applicable), easier refactors.

### Rule 1.2 — OS/QEMU paths require `no_std` (+ `alloc` when needed)

- **Kernel** and **OS/QEMU services** must compile in bare-metal mode: `no_std` and (only if required) `alloc`.
- Dependency hygiene rules are owned by **RFC‑0009**.

### Rule 1.3 — Feature gating is mandatory

- OS services must build with: `--no-default-features --features os-lite`.
- Any crate that supports both host and OS must clearly separate host (`std`) and OS (`os-lite`) paths.

---

## 2) Layered code philosophy (what belongs where)

### 2.1 Kernel (`source/kernel/neuron/`)

**Goal**: Minimal, deterministic, capability-driven kernel. No policy, no crypto, no protocol parsing.

- **Correctness first**: the kernel must be warning-clean in OS builds (`deny(warnings)` is intentional).
- **Determinism**: proofs are marker-driven; avoid timing-luck behavior.
- **Concurrency model**: follow the Servo-inspired ownership/message-passing guidance in `docs/architecture/16-rust-concurrency-model.md`.
- **No “business logic”**: kernel owns scheduling/vm/ipc/capability mechanics only.

### 2.2 Core libraries (contract crates)

- Prefer **small, composable crates** with explicit error types and bounded inputs.
- Default to `#![forbid(unsafe_code)]` unless the crate is explicitly a low-level backend.

### 2.3 OS services (`source/services/*d`)

**Goal**: Thin adapters over userspace libraries.

- No `unwrap`/`expect` on untrusted inputs.
- Markers must be “honest green”: `*: ready` only after real readiness; `SELFTEST: ... ok` only after real behavior.
- Heavy logic should move into `userspace/` crates that have host tests.

### 2.4 Userspace libraries (`userspace/`)

- Host-first: tests and determinism come first.
- OS backends should be explicit and must not “fake” OS support (return `Unsupported` deterministically unless implemented).

### 2.5 Tests

- Prefer host tests for behavior and negative cases.
- QEMU tests are bounded smoke checks with deterministic marker ordering (`scripts/qemu-test.sh`).

---

## 3) Kernel lint + warning policy (Rust-conform and change-friendly)

### Rule 3.1 — `deny(warnings)` in kernel OS builds is a feature, not a nuisance

We use warning-clean builds as a **drift detector**. Warnings in the kernel tend to indicate:

- dead code that hides incomplete plumbing,
- accidental feature-path changes,
- or incomplete refactors.

### Rule 3.2 — `dead_code` handling in kernel

`dead_code` is valuable, but there are legitimate bring-up phases where kernel-internal APIs are staged.

**Best practice order**:

1. **Prefer real usage** when it reflects actual invariants (best).
2. If usage would be artificial or would pull more code into the kernel, use a **targeted suppression** with a removal clause.

**Allowed form** (tight scope only):

- `#[allow(dead_code)]` on the **smallest item** (function/const), never the whole module, plus:
  - a short “why”, and
  - **REMOVE_WHEN(...)** clause referencing the owning task/landing point.

**Not allowed**:

- blanket `#![allow(dead_code)]` on kernel modules (except narrowly-scoped bring-up stubs where the module is explicitly
  tagged as such and scheduled for removal).

### Decision for the current `Asid` / `AsHandle` case

For `Asid::{from_raw, raw, KERNEL}` and `AsHandle::{from_raw, raw}`:

- **We choose targeted suppression + removal clause**, because “using them” right now would be artificial and risks
  changing kernel plumbing semantics prematurely.
- Removal trigger should be the address-space plumbing work described in `tasks/TASK-0011B-kernel-rust-idioms-pre-smp.md`
  and/or the AddressSpaceManager syscall wiring.

---

## 4) Unsafe policy (kernel and low-level backends)

### Rule 4.1 — Unsafe is permitted only where necessary, and must be auditable

- Prefer safe Rust. Use `unsafe` for:
  - MMIO/CSR reads/writes,
  - context switching,
  - page-table manipulation,
  - trap entry/exit glue.

### Rule 4.2 — Keep unsafe blocks small and document invariants

- “Small unsafe, big safe”:
  - Do the minimal raw pointer operation in `unsafe`,
  - immediately convert into safe types/structures.

### Rule 4.3 — No `unsafe impl Send/Sync` without a written safety argument

- Prefer deriving `Send/Sync` automatically where possible.
- If you must add `unsafe impl Send/Sync`, include a comment describing:
  - what data is shared,
  - what invariants make it safe,
  - and how it is enforced (types, ownership, or explicit synchronization).

---

## 5) Error handling and panics

- In kernel and OS services: prefer explicit error propagation.
- Avoid `unwrap`/`expect` on untrusted inputs.
- Panics are reserved for truly unreachable kernel invariants and should be rare; prefer “fail closed” behavior in
  security-sensitive paths.

---

## 6) Type-driven safety (TASK-11B principles)

### Rule 6.1 — Use newtypes to prevent handle confusion

**Problem**: Raw types (`u32`, `u64`, `usize`) are easy to mix up, especially for handles, IDs, and slots.

**Solution**: Wrap in `#[repr(transparent)]` newtypes:

```rust
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
```

**When to use**:
- Kernel handles: `Pid`, `AsHandle`, `CapSlot`
- Service-specific identifiers: `RecordId`, `BundleId`, `SessionId`
- Timestamps: `TimestampNsec`, `DeadlineNsec`
- Capability indices, file descriptors, network ports (where confusion is likely)

**Benefits**:
- Compile-time prevention of argument-order bugs
- Self-documenting APIs
- Zero runtime cost (`#[repr(transparent)]`)

### Rule 6.2 — Use phantom types for compile-time type safety

For types with runtime tags (e.g., capabilities, resources), consider phantom type parameters:

```rust
use core::marker::PhantomData;

/// Capability type marker (zero-sized)
pub trait CapabilityType {
    const KIND: CapabilityKind;
}

pub struct Endpoint;
impl CapabilityType for Endpoint {
    const KIND: CapabilityKind = CapabilityKind::Endpoint;
}

/// Typed capability (zero-cost abstraction)
#[repr(transparent)]
pub struct Capability<T: CapabilityType> {
    inner: UntypedCapability,
    _marker: PhantomData<T>,
}

// Now type-safe!
fn send_to_endpoint(cap: Capability<Endpoint>) { /* ... */ }
```

**When to use**:
- Capabilities with distinct kinds (Endpoint, Vmo, Mmio)
- Resources with type-specific operations (File handles, sockets)
- State machines where states should be compile-time distinct

**Trade-offs**:
- More complex type signatures
- Requires runtime check at construction (`from_untyped`)
- Use sparingly: prefer newtypes for most cases

---

## 7) Ownership model documentation (explicit is better than implicit)

### Rule 7.1 — Document ownership boundaries in module comments

For kernel modules and complex services, add an "Ownership Model" section:

```rust
//! ## Ownership Model (Rust-specific)
//!
//! ### Global State
//! - `KERNEL_STATE: MaybeUninit<KernelState>` — Owned by kernel, initialized once
//!
//! ### KernelState fields
//! - `scheduler: Scheduler` — Owns all TaskTable entries
//! - `tasks: TaskTable` — Owns Task structs, borrows to Scheduler
//! - `ipc: Router` — Owns message queues, borrows to syscall handlers
//!
//! ### Lifetimes
//! - Most kernel structures have 'static lifetime (never deallocated)
//! - Temporary borrows in syscall handlers (e.g., &mut Scheduler)
```

**When to document**:
- Kernel modules with global state
- Services with complex shared state (ring buffers, caches)
- Any time ownership is non-obvious from types alone

### Rule 7.2 — Use `!Send` and `!Sync` markers to prevent accidental sharing

For single-threaded structures (kernel before SMP, OS services):

```rust
/// Scheduler state (single-threaded until SMP)
pub struct Scheduler {
    queues: [VecDeque<Pid>; 4],
    current: Option<Pid>,
    _not_send_sync: PhantomData<*const ()>, // Explicitly !Send + !Sync
}
```

**When to use**:
- Kernel structures that will be per-CPU in SMP (scheduler, allocators)
- OS services that are inherently single-threaded
- Structures with raw pointers or other !Send internals

**For SMP-safe structures**, add explicit `unsafe impl Send/Sync` with safety comments:

```rust
/// HAL machine state (immutable after init, safe to share)
pub struct VirtMachine {
    uart_base: usize,
    timer_freq: u64,
}

// Safety: VirtMachine is immutable after initialization.
// All fields are plain integers (no raw pointers).
unsafe impl Send for VirtMachine {}
unsafe impl Sync for VirtMachine {}
```

---

## 8) Error handling discipline (must-use and propagation)

### Rule 8.1 — Security-critical errors must be `#[must_use]`

All error types that represent security decisions or resource exhaustion MUST have `#[must_use]`:

```rust
/// Syscall result (internal representation)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "syscall errors must be handled"]
pub enum SyscallError {
    PermissionDenied,
    InvalidArgument,
    NotFound,
    OutOfMemory,
}
```

**When to use**:
- Kernel syscall errors
- Capability checks (permission denials)
- Authentication/authorization results
- Resource allocation failures (OOM, buffer full)
- Security violations (overflow, malformed input)

### Rule 8.2 — Prefer `Result<T, E>` over numeric error codes

**Internal APIs** should use `Result<T, E>`:

```rust
// Good: type-safe, composable with ?
fn validate_entry(pc: usize) -> Result<(), SpawnError> {
    if pc < TEXT_START {
        return Err(SpawnError::InvalidEntryPoint);
    }
    Ok(())
}

pub fn sys_spawn(...) -> SyscallResult<Pid> {
    let entry_pc = validate_entry(args.pc)?; // ? operator!
    let task = scheduler.spawn(entry_pc)?;
    Ok(task.pid)
}
```

**Syscall ABI** (userspace-facing) still returns `isize` (POSIX compatibility), but conversion is explicit:

```rust
impl SyscallError {
    /// Convert to POSIX errno (for userspace ABI)
    pub const fn to_errno(self) -> isize {
        match self {
            Self::PermissionDenied => -libc::EPERM as isize,
            Self::InvalidArgument => -libc::EINVAL as isize,
            // ...
        }
    }
}
```

---

## 9) Service IPC contracts (hybrid approach for consistency)

### Rule 9.1 — Services SHOULD use hybrid IPC contracts

**Pattern** (established by `samgrd`, `bundlemgrd`, `keystored`, `policyd`, `vfsd`, `packagefsd`, `execd`):

```toml
# service/Cargo.toml
[features]
default = ["std", "idl-capnp"]
std = ["dep:capnp", "dep:nexus-idl-runtime", "nexus-ipc/std"]
idl-capnp = ["nexus-idl-runtime/capnp", "dep:capnp"]
os-lite = ["dep:nexus-abi", "nexus-ipc/kernel-ipc"]
```

**Implementation**:
- `src/std_server.rs`: Cap'n Proto handlers (host tests, type-safe)
- `src/os_lite.rs`: Versioned byte frames (OS/QEMU, minimal overhead)
- `tools/nexus-idl/schemas/<service>.capnp`: Schema documentation

**Wire protocol (OS)** example:

```rust
// src/os_lite.rs
const MAGIC0: u8 = b'X';  // Service-specific magic
const MAGIC1: u8 = b'Y';
const VERSION: u8 = 1;

const OP_FOO: u8 = 1;
const OP_BAR: u8 = 2;
// Frame: [MAGIC0, MAGIC1, VERSION, OP, ...payload]
```

**When to deviate**:
- ⚠️ If a service is **host-only** (e.g., tooling), Cap'n Proto only is acceptable
- ⚠️ If a service has **extremely high throughput** and profiling shows Cap'n Proto overhead is significant on host, consider byte frames for both (but keep IDL schema for docs)

### Rule 9.2 — Bound all inputs in wire protocols

**Security invariant**: All input sizes must be bounded and checked before allocation.

```rust
// Good: explicit bounds
const MAX_SCOPE_LEN: usize = 64;
const MAX_MSG_LEN: usize = 256;
const MAX_FIELDS_LEN: usize = 512;

fn decode_append(frame: &[u8]) -> Result<AppendRequest, DecodeError> {
    if frame.len() < MIN_FRAME_LEN {
        return Err(DecodeError::TooShort);
    }
    
    let scope_len = frame[4] as usize;
    if scope_len > MAX_SCOPE_LEN {
        return Err(DecodeError::OversizedScope);
    }
    
    // ... safe to proceed
}
```

**When to enforce**:
- All OS service IPC handlers
- Kernel IPC frame parsing
- Network protocol decoders

---

## 10) References (project-local)

- **TASK-11B** (Kernel Rust idioms): `tasks/TASK-0011B-kernel-rust-idioms-pre-smp.md`
- **Servo-inspired concurrency model**: `docs/architecture/16-rust-concurrency-model.md`
- **Host-first/QEMU-last testing**: `docs/architecture/02-selftest-and-ci.md`, `scripts/qemu-test.sh`
- **no_std dependency hygiene**: `docs/rfcs/RFC-0009-no-std-dependency-hygiene-v1.md`
- **Service architecture**: `docs/adr/0017-service-architecture.md`
- **IPC runtime**: `docs/adr/0003-ipc-runtime-architecture.md`
