# NEURON Microkernel (v0 Increment 1)

The first NEURON milestone focuses on a minimal, well documented
microkernel capable of booting on the QEMU RISC-V `virt` machine,
printing a banner over UART and exposing a deterministic syscall
surface for early user tasks.

## Security north star (system-level)

NEURON is the kernel core of a HarmonyOS-like, Rust-first, RISC‑V-first system.
The system security roadmap is intentionally hybrid:

- Verified boot + signed bundles/packages + capability-based isolation as the MVP root.
- Pluggable key custody via `keystored`/`identityd` that can later use Secure Element / TEE
  without rewriting kernel interfaces.
- Measured boot/attestation later for distributed trust (`softbusd`) without inflating kernel TCB.

## Boot Flow

1. `_start` is provided by `boot.rs`. It clears `.bss`, installs the
   trap vector and jumps into `kmain::kmain`.
2. `kmain` instantiates the HAL (`VirtMachine`), scheduler, capability
   table, IPC router, address space and syscall table.
3. The UART banner `NEURON` is emitted via the boot UART, proving that
   early MMIO is working.

## Syscall Surface

The syscall surface is intentionally small but evolves during bring-up.
The authoritative list (including numeric IDs) lives in `source/kernel/neuron/src/syscall/mod.rs`.

- **0 `yield`**: Rotate the scheduler and return the next runnable task id. Activates the target task's address space.
- **1 `nsec`**: Return the monotonic time in nanoseconds derived from the `time` CSR.
- **2 `send`**: Send an IPC message via an endpoint capability.
- **3 `recv`**: Receive the next pending IPC message.
- **4 `map`**: Map a page from a VMO capability into the caller's active address space.
- **5 `vmo_create`**: Create a VMO capability.
- **6 `vmo_write`**: Write bytes into a VMO capability.
- **7 `spawn`**: Create a child task (fresh Sv39 AS by default) with a guarded stack.
- **8 `cap_transfer`**: Duplicate/grant a capability to another task with a rights mask (subset-only).
- **9 `as_create`**: Allocate a new Sv39 address space and return its opaque handle.
- **10 `as_map`**: Map a VMO into a *target* address space identified by handle. Enforces W^X at the syscall boundary.
- **11 `exit`**: Terminate the current task.
- **12 `wait`**: Wait for a child task exit.
- **13 `exec`**: Execute an ELF payload (loader path).
- **14 `ipc_send_v1`**: Kernel IPC v1 send (payload copy-in) (see RFC‑0005).
- **18 `ipc_recv_v1`**: Kernel IPC v1 recv (payload copy-out) (see RFC‑0005).
- **19 `ipc_endpoint_create`**: Create a kernel IPC endpoint (privileged) (see RFC‑0005).
- **27 `mmio_map`**: Map a device MMIO capability window into the caller AS (USER+RW, never EXEC).
- **28 `cap_query`**: Query a capability slot (kind/base/len) into a user buffer (driver bring-up primitive).

Errors follow the conventional POSIX encoding: handlers return
`-errno` (two's complement) in `a0`. Key codes used by the current
increment:

- `EPERM` for capability or W^X violations.
- `EINVAL` for malformed arguments and IPC routing failures.
- `ENOSPC` when the ASID allocator is exhausted.
- `ENOSYS` for disabled/unsupported functionality.
- `ENOMEM` when the guarded stack pool runs out of pages.

> Current state note (2025-12-18): syscall handlers return `-errno` in `a0` for
> expected errors. The kernel may still terminate tasks in true “no forward
> progress” situations (e.g. repeated ECALL storms), but ordinary syscall errors
> are returned to userspace.

## Address Space Model

- Sv39 translation with three levels of page tables. Intermediate tables are allocated lazily as
  mappings are installed via `AddressSpaceManager::map_page`.
- The ASID allocator tracks 256 slots (ASID `0` is reserved for the kernel). Handles returned by
  `SYS_AS_CREATE` wrap the internal slot index and remain opaque to callers.
- Fresh address spaces are seeded with a kernel identity map using final-image linker symbols:
  `[__text_start..__text_end)` is mapped RX|GLOBAL, `[__text_end..__bss_end)` RW|GLOBAL, a
  dedicated kernel stack RW|GLOBAL with a bottom guard page left unmapped, a private selftest stack
  bracketed by guards, and the UART window RW|GLOBAL. GLOBAL keeps kernel pages visible across
  ASID switches.
- Kernel mapping finishes by emitting a single `map kernel segments ok` UART marker once the linker
  ranges have been installed. The SATP switch island performs an eight-byte RX-sanity sample around
  the current PC (panicking on all-zero fetch windows) before writing SATP, switches stacks inside
  the identity-mapped page, and prints `AS: post-satp OK` after the TLB fence to prove the return
  path stayed within the island.
- Each address space maintains the set of owning tasks so the manager can reject destruction while
  references remain. Activating a handle writes SATP and issues a global `sfence.vma`.

## W^X Policy

- Writable and executable user mappings are mutually exclusive. `SYS_AS_MAP` rejects requests that
  combine `PROT_WRITE` and `PROT_EXEC` and returns `EPERM`.
- The policy applies uniformly to mappings requested by the caller and those created by kernel
  helpers (for example, guarded stacks installed during `spawn`).

## BootstrapMsg (child bootstrap payload)

The kernel sends a single bootstrap message to the child's seeded endpoint on `spawn`.
The payload layout is stable and `#[repr(C)]`:

```rust
#[repr(C)]
pub struct BootstrapMsg {
    pub argc: u32,
    pub argv_ptr: u64,   // child VA (string table); 0 in MVP
    pub env_ptr: u64,    // child VA; 0 in MVP
    pub cap_seed_ep: u32,// initial endpoint handle granted to the child
    pub flags: u32,      // reserved
}
```

Golden layout tests assert size/padding correctness.

## Spawn semantics (dedicated address spaces)

- Default behaviour: a zero `as_handle` argument instructs the kernel to create a fresh Sv39
  address space for the child. The kernel maps a four-page RW stack capped by an unmapped guard
  page and activates the new AS during scheduling.
- Custom handle: callers may bind the child to an existing address space by passing a non-zero
  handle obtained via `SYS_AS_CREATE`. The caller is responsible for provisioning the stack in
  that address space.
- Entry checks: `entry_pc` must lie within `__text_start..__text_end` and be aligned; otherwise
  `SpawnError::InvalidEntryPoint` is raised.
- Cap table: the child receives a copy of the parent's provided bootstrap endpoint into slot `0`
  (rights are intersected with the mask).
- Bootstrap: the kernel enqueues one IPC to endpoint `0` with a zeroed `BootstrapMsg` payload.
- Trapframe: the child resumes in S/U-mode at `entry_pc` with `sp` pointing at the guarded stack
  top (or the caller-provided stack pointer when using a custom address space).

## Stage policy and selftests (OS path)

- Early boot forbids heavy formatting/allocations; only raw UART writes until selftests run.
- Selftests execute on a private, guarded stack (RW pages bracketed by unmapped guards); timer IRQs
  are masked during the run.
- UART markers (subset): `KSELFTEST: as create ok` → `KSELFTEST: as map ok` →
  `KSELFTEST: child newas running` → `KSELFTEST: spawn newas ok` → `KSELFTEST: w^x enforced`.
- Bring-up diagnostics: illegal-instruction traps print `sepc/scause/stval` and instruction bytes;
  optional `trap_symbols` resolves `sepc` to `name+offset`. A post-SATP marker verifies return.
- Feature gates:
  - Default: `boot_banner`, `selftest_priv_stack`, `selftest_time`.
  - Optional: `selftest_ipc`, `selftest_caps`, `selftest_sched`, `trap_symbols`, `trap_ring`,
    `debug_stack_guards`.

## Structured logging

- Kernel diagnostics flow through lightweight log macros that annotate each line with a severity
  and `target` module: `[INFO mm] map kernel segments ok`.
- `ERROR`, `WARN`, and `INFO` logs are always emitted; `DEBUG`/`TRACE` only compile in debug builds
  (`debug_assertions`) to avoid noise on production runs.
- Required acceptance markers (e.g., `KSELFTEST: …`, `AS: post-satp OK`) remain intact as part of
  the structured messages so CI can continue to grep for them.

## Trap symbolization (opt-in)

When the `trap_symbols` feature is enabled, the build script emits a compact
`TRAP_SYMBOLS: &[(usize, &str)]` table into `.rodata`. Illegal-instruction logs
lookup the nearest symbol to `sepc` and print `name+offset` for debugging. This
has zero runtime overhead when the feature is disabled.

## IPC Header

NEURON exchanges messages using a fixed 16 byte header declared in
`ipc::header::MessageHeader`:

```text
+-------+-------+------+--------+-----+
| src:u32 | dst:u32 | ty:u16 | flags:u16 | len:u32 |
+-------+-------+------+--------+-----+
```

Payload bytes are stored inline in the queue and truncated to `len`
bytes when the message is created.

## Capability Invariants

- Every capability belongs to exactly one task-local table.
- Derivation intersects rights with the parent capability. Rights can
  never be escalated.
- Endpoint capabilities must contain the `SEND` or `RECV` right to
  access queues. VMO capabilities require the `MAP` right to install
  mappings.
- Capability slots are pre-sized per task (32 entries for the bootstrap
  task).

## Scheduler Overview

The scheduler implements a round-robin policy with QoS hints. Tasks are
queued in four buckets (`Idle`, `Normal`, `Interactive`, `PerfBurst`).
When `yield` is invoked the current task is placed at the tail of its
bucket and the highest priority non-empty bucket is dequeued.

### Ownership Model (Rust-Specific)

**Who owns what?**

- **`Scheduler`**: Owns the task queues (`VecDeque<Task>`) and current task state
  - **Ownership**: Exclusive mutable access (`&mut self` methods)
  - **Lifetime**: Lives in `KERNEL_STATE` (static lifetime)
  - **Thread safety**: Single-CPU (v1), per-CPU (SMP v2)

- **`TaskTable`**: Owns all `TaskEntry` structs (PID → task metadata)
  - **Ownership**: Exclusive mutable access to task lifecycle
  - **Lifetime**: Static (kernel global)
  - **Invariant**: Only `TaskTable` can spawn/exit tasks

- **`AddressSpaceManager`**: Owns all page tables and ASID allocator
  - **Ownership**: Exclusive mutable access to memory mappings
  - **Lifetime**: Static (kernel global)
  - **Invariant**: Only `AddressSpaceManager` can modify page tables

- **`IpcRouter`**: Owns all endpoint queues
  - **Ownership**: Shared mutable access (interior mutability via locks)
  - **Lifetime**: Static (kernel global)
  - **Invariant**: Only `IpcRouter` can enqueue/dequeue messages

**Borrowing Rules**:

- Syscall handlers borrow `&mut` references to kernel subsystems
- No aliasing: only one subsystem can be borrowed mutably at a time
- Trap handler coordinates borrows (owns all subsystems transitively)

**SMP Implications (TASK-0012)**:

- **Per-CPU Scheduler**: Each CPU owns its local runqueue (no sharing)
- **Shared TaskTable**: Protected by spinlock (short critical sections)
- **Shared AddressSpaceManager**: Protected by spinlock (page faults only)
- **Shared IpcRouter**: Lock-free queues (atomic operations)

See `docs/architecture/16-rust-concurrency-model.md` for detailed SMP ownership design.

### Implemented vs. Aspirational (avoid doc drift)

This section mixes **current structure** and **planned SMP structure**. To keep the docs honest:

- **Implemented today (single-hart bring-up)**:
  - single `Scheduler` instance,
  - trap/syscall path is effectively single-threaded,
  - invariants are proven via deterministic marker/selftests.

- **Planned for SMP v1/v2 (tasks)**:
  - per-CPU scheduler ownership (TASK-0012),
  - explicit locking/atomic boundaries for shared state (TASK-0277 policy),
  - affinity/shares/QoS controls (TASK-0042, TASK-0013) gated by `policyd`.

When implementation differs from the plan, the task (not this doc) is the authority; update this section
once the implementation lands.

### Construction authority (Rust handles / newtypes)

Rust newtypes are only maximally useful when it is clear **who is allowed to construct them** (authority),
and when the API makes incorrect construction hard:

- **`Pid`**: constructed by `TaskTable` only (kernel authority).
- **`Asid`**: allocated/freed by `AddressSpaceManager` only.
- **`AsHandle`**: created by the `as_create` syscall path; opaque to callers; never reveals the ASID.
- **`CapSlot`**: indexes a per-task cap table; validated at syscall boundaries.

See `source/kernel/neuron/src/types.rs` for the current newtypes and comments, and keep constructors scoped
so invariants stay enforceable.

## HAL Snapshot

The HAL targets RISC-V `virt` and exposes traits for timers, UART, MMIO,
IRQ control and TLB invalidation. `VirtMachine` bundles the concrete
implementations used by the kernel.

## Testing Strategy

- Host-based unit tests validate message header layout, scheduler
  ordering and syscall send/recv semantics using the in-memory router.
- `just qemu` (backed by `scripts/run-qemu-rv64.sh`) launches
  `qemu-system-riscv64` with the freshly built kernel archive to confirm
  the boot banner and trap setup execute without crashing.

For deterministic QEMU acceptance (marker contract + ordering), use the canonical harness:

- `scripts/qemu-test.sh` (contract implementation)
- `docs/testing/index.md` (methodology + marker guidance)
