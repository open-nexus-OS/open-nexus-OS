# NEURON Microkernel (v0 Increment 1)

The first NEURON milestone focuses on a minimal, well documented
microkernel capable of booting on the QEMU RISC-V `virt` machine,
printing a banner over UART and exposing a deterministic syscall
surface for early user tasks.

## Boot Flow

1. `_start` is provided by `boot.rs`. It clears `.bss`, installs the
   trap vector and jumps into `kmain::kmain`.
2. `kmain` instantiates the HAL (`VirtMachine`), scheduler, capability
   table, IPC router, address space and syscall table.
3. The UART banner `NEURON` is emitted via the boot UART, proving that
   early MMIO is working.

## Syscall Surface

| Number | Symbol          | Description |
| ------ | --------------- | ----------- |
| 0      | `yield`         | Rotate the scheduler and return the next runnable task id. |
| 1      | `nsec`          | Return the monotonic time in nanoseconds derived from the `time` CSR. |
| 2      | `send`          | Send an IPC message via an endpoint capability. |
| 3      | `recv`          | Receive the next pending IPC message. |
| 4      | `map`           | Map a page from a VMO capability into the caller address space. |
| 7      | `spawn`         | Create a child task (MVP: shared address space). PC/SP set from args, seed cap table, enqueue `BootstrapMsg`. |
| 8      | `cap_transfer`  | Duplicate/grant a capability to another task with a rights mask (subset-only). |

Errors are reported via negative sentinel values (`usize::MAX`
descending) in `a0`:

- `usize::MAX`: invalid syscall number
- `usize::MAX - 1`: capability lookup or rights failure
- `usize::MAX - 2`: IPC routing error
- `usize::MAX - 3`: spawn error
- `usize::MAX - 4`: capability transfer error

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

## Spawn (MVP) semantics

- Address space: shared with parent (temporary rule for bring-up).
- Entry checks: `entry_pc` must lie within `__text_start..__text_end` and be aligned; otherwise `SpawnError::InvalidEntryPoint`.
- Stack: `stack_sp == 0` permitted in MVP; dedicated stacks land in a later phase.
- Cap table: child receives a copy of the parent's provided bootstrap endpoint into slot `0` (rights are intersected with the mask).
- Bootstrap: kernel enqueues one IPC to endpoint `0` with a zeroed `BootstrapMsg` payload.
- Trapframe: child resumes in S/U-mode at `entry_pc` with `sp = stack_sp`.

## Stage policy and selftests (OS path)

- Early boot forbids heavy formatting/allocations; only raw UART writes until selftests run.
- Selftests execute on a private, canaried stack; timer IRQs are masked during the run.
- UART markers (subset): `SELFTEST: begin` → `SELFTEST: time ok` → `KSELFTEST: spawn ok` → `SELFTEST: end`.
- Feature gates:
  - Default: `boot_banner`, `selftest_priv_stack`, `selftest_time`.
  - Optional: `selftest_ipc`, `selftest_caps`, `selftest_sched`, `trap_symbols`.

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
