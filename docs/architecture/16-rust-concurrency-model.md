# Rust Concurrency Model (Servo-inspired)

**Created**: 2026-01-09  
**Owner**: @kernel-team  
**Status**: Active guidance; TASK-0012 SMP v1 baseline, TASK-0012B hardening, and TASK-0013 QoS/timed v1 implemented (SMP v2+ follow-ups pending)

---

## Overview

NEURON leverages Rust's **fearless concurrency** model, inspired by the [Servo browser engine](https://servo.org/)'s
parallel layout and styling architecture. This document defines how we use Rust's ownership system to achieve
**safe parallelism with auditable synchronization** in the kernel.

Important: **SMP v1 is not a “lock-free kernel” project.** The v1 target is:

- per-CPU ownership by default,
- a small number of short, auditable critical sections where sharing is unavoidable,
- deterministic proofs via KSELFTEST markers (results, not timing).

Lock-free techniques (or “unsafe Send/Sync”) are treated as **optional follow-ups** only after correctness is
proven and measured.

---

## Servo's Lessons for Kernel Design

### 1. **Data-Parallel Work Stealing** (Servo's Layout Engine)

**Servo approach**:

- Layout tree is immutable during parallel traversal
- Each worker thread owns a disjoint subtree
- No locks needed (ownership guarantees no aliasing)

**NEURON equivalent** (for SMP scheduler):

```rust
// Each CPU owns its runqueue (no shared state)
pub struct PerCpuScheduler {
    local_queue: VecDeque<Pid>,  // Owned by this CPU
    cpu_id: usize,
    _not_send: PhantomData<*const ()>, // Explicitly !Send
}

// Work stealing via message passing (not shared memory)
pub enum SchedulerMsg {
    StealWork { from_cpu: usize, count: usize },
    MigrateTask { task: Pid, to_cpu: usize },
}
```

**Key insight**: Ownership prevents data races at **compile time**, not runtime.

---

### 2. **Message Passing over Shared Memory** (Servo's IPC)

**Servo approach**:

- Components communicate via typed channels (`crossbeam::channel`)
- No shared mutable state between threads
- Ownership transferred via `send()`

**NEURON equivalent** (for inter-CPU communication):

```rust
// Inter-Processor Interrupt (IPI) as message passing
pub struct IpiMessage {
    kind: IpiKind,
    payload: IpiPayload,
}

pub enum IpiKind {
    TlbShootdown { asid: AsHandle, vaddr: usize },
    WakeTask { task: Pid },
    Reschedule,
}

// Sender transfers ownership (no Copy)
pub fn send_ipi(target_cpu: usize, msg: IpiMessage) {
    // msg is moved, sender can't access it anymore
    unsafe {
        IPI_MAILBOX[target_cpu].push(msg);
        trigger_ipi_interrupt(target_cpu);
    }
}
```

**Key insight**: `Send` ensures only thread-safe data crosses CPU boundaries.

**Kernel-specific note**: For v1 SMP bring-up, the “message passing” mechanism may be implemented as a small,
bounded mailbox + IPI signal with a simple lock/atomic protocol. The goal is auditable correctness, not
maximal throughput on day 1.

---

### 3. **Immutable Shared State** (Servo's Style System)

**Servo approach**:

- CSS rules are immutable after parsing
- Multiple threads can read simultaneously (`Arc<StyleRule>`)
- No locks needed (immutable = thread-safe)

**NEURON equivalent** (for HAL and read-only kernel data):

```rust
// HAL machine state is immutable after boot
pub struct VirtMachine {
    uart_base: usize,
    timer_freq: u64,
    // No interior mutability (no Mutex, no RefCell)
}

// Safe to share across CPUs *if and only if* the safety contract is upheld.
//
// Prefer letting the compiler auto-derive Send/Sync where possible.
// Only use `unsafe impl Send/Sync` when necessary, and document:
// - why it cannot be derived automatically,
// - what invariant makes it safe,
// - how that invariant is enforced.
unsafe impl Send for VirtMachine {}
unsafe impl Sync for VirtMachine {}

// Usage: Can be borrowed by all CPUs simultaneously
static MACHINE: OnceCell<VirtMachine> = OnceCell::new();

pub fn get_machine() -> &'static VirtMachine {
    // In kernel code, avoid using `expect`/`unwrap` patterns as a design example.
    // The real implementation should either:
    // - prove initialization by construction (boot ordering), or
    // - return a Result/Option and handle failure deterministically.
    MACHINE.get().expect("HAL not initialized")
}
```

**Key insight**: Immutability eliminates entire classes of concurrency bugs.

---

### 4. **Per-Thread Ownership** (Servo's Layout Workers)

**Servo approach**:

- Each worker thread owns its allocator
- No global allocator lock contention
- Thread-local storage for hot paths

**NEURON equivalent** (for per-CPU kernel state):

```rust
// Per-CPU kernel state (no sharing between CPUs)
#[repr(C, align(64))] // Cache-line aligned to prevent false sharing
pub struct PerCpuState {
    cpu_id: usize,
    scheduler: PerCpuScheduler,
    current_task: Option<Pid>,
    irq_depth: usize, // IRQ nesting level
    // Each CPU has its own stack allocator
    stack_pool: StackAllocator,
}

// Array of per-CPU states (indexed by CPU ID)
static PER_CPU: [PerCpuState; MAX_CPUS] = [/* ... */];

// Accessor: Returns mutable reference to current CPU's state
// Safety: Each CPU only accesses its own slot (no aliasing)
pub fn current_cpu_state() -> &'static mut PerCpuState {
    let cpu_id = read_cpu_id(); // Hardware register
    unsafe { &mut PER_CPU[cpu_id] }
}
```

**Key insight**: Ownership partitioning eliminates lock contention.

---

## Rust Concurrency Primitives for NEURON

### 1. **Send and Sync Traits**

```rust
// Send: Can be transferred between threads (ownership transfer)
// Sync: Can be shared between threads (immutable or internally synchronized)

// Example: Task structure
pub struct Task {
    pid: Pid,
    state: TaskState,
    // ...
}

// Task is Send (can be migrated between CPUs)
// Task is NOT Sync (mutable, needs exclusive access)
unsafe impl Send for Task {}
// Note: Sync is NOT implemented (would require interior mutability)
```

**Decision matrix**:

- **`Task`**: **Send ✅**, **Sync ❌** — can migrate CPUs, but needs exclusive access
- **`VirtMachine`**: **Send ✅**, **Sync ✅** — immutable after init, safe to share
- **`PerCpuScheduler`**: **Send ❌**, **Sync ❌** — CPU-local, never crosses boundaries
- **`Capability`**: **Send ✅**, **Sync ❌** — can be transferred, but not shared
- **`IpiMessage`**: **Send ✅**, **Sync ❌** — sent between CPUs, consumed on receipt

---

### 2. **Atomics for Lock-Free Coordination**

```rust
use core::sync::atomic::{AtomicUsize, Ordering};

// Global task counter (lock-free)
static NEXT_PID: AtomicUsize = AtomicUsize::new(1);

pub fn allocate_pid() -> Pid {
    let raw = NEXT_PID.fetch_add(1, Ordering::Relaxed);
    Pid::from_raw(raw as u32)
}

// CPU-local flag (no contention)
static CPU_ONLINE: [AtomicBool; MAX_CPUS] = [/* ... */];

pub fn mark_cpu_online(cpu_id: usize) {
    CPU_ONLINE[cpu_id].store(true, Ordering::Release);
}
```

**Ordering guidelines**:

- `Relaxed`: Counters, statistics (no synchronization needed)
- `Acquire`/`Release`: Flag-based coordination (e.g., CPU online status)
- `SeqCst`: Rare (only when total ordering required, e.g., shutdown sequence)

---

### 3. **Spin Locks (Minimal Use)**

```rust
use spin::Mutex;

// Only for truly shared mutable state (rare in NEURON)
pub struct GlobalIpcRouter {
    // Shared message queues (all CPUs can send/recv)
    queues: Mutex<BTreeMap<Pid, VecDeque<Message>>>,
}

impl GlobalIpcRouter {
    pub fn send(&self, dst: Pid, msg: Message) -> Result<(), IpcError> {
        let mut queues = self.queues.lock(); // Short critical section
        queues.get_mut(&dst)
            .ok_or(IpcError::NoSuchTask)?
            .push_back(msg);
        Ok(())
    }
}
```

**Lock hierarchy (NORMATIVE, implemented — SMP track A2, TASK-0277):**

1. **Tier 1 — `KERNEL_LOCK` (the kernel BKL)**: one IRQ-safe `SpinIrqLock`
   (`core/trap.rs`) guarding the shared aggregate (`TaskTable`, `ipc::Router`,
   `AddressSpaceManager` incl. ASID allocator, `WaitsetTable`, `FenceTable`,
   timer tables). Every mutable materialization of kernel handles goes through
   `trap::KernelGuard`, which holds this lock for the FULL borrow duration.
2. **Tier 2 — per-CPU run-queue locks** (ascending CPU index when scanning
   multiple, e.g. work stealing).
3. **Tier 3 — leaf atomics** (online mask, counters, PT-pool cursor, …).

**Rules:**
- Acquire strictly downward (BKL → run queue → atomics). NEVER acquire the
  BKL while holding a run-queue lock.
- `SpinIrqLock` is the ONLY lock type permitted on trap-reachable paths — it
  masks SIE before acquisition, so a trap on the holding hart can never
  re-enter the lock.
- NO guard may be held across a context switch that leaves via `sret`. The
  schedule decision stages the chosen frame into the hart-local
  `HartLocal.resume_frame` slot under the BKL, drops the guard, and only then
  switches (see `kmain::idle_loop_bkl`).
- Fault diagnostics on the way to `panic!` read handles lock-free
  (`runtime_kernel_handles_diagnostic`) and must never mutate — a kernel page
  fault inside a syscall would otherwise deadlock on the BKL it already holds.
- RISC-V TRAP (empirical, SMP track A2c): an AMO `fetch_add` on the PT-pool
  cursor reproducibly wedged the boot; the pool uses a bounded lr/sc CAS loop
  instead (same primitive class as the spin locks). Prefer lr/sc-based RMW in
  early-boot/mm paths until the AMO behavior is root-caused.

---

### 4. **Message Passing (IPI Mailboxes)**

```rust
// Lock-free SPSC queue (Single Producer, Single Consumer)
// Each CPU has a mailbox that only it reads from
pub struct IpiMailbox {
    queue: ArrayQueue<IpiMessage, 16>, // Bounded lock-free queue
}

static IPI_MAILBOXES: [IpiMailbox; MAX_CPUS] = [/* ... */];

// Send IPI from any CPU to target CPU
pub fn send_ipi(target_cpu: usize, msg: IpiMessage) {
    IPI_MAILBOXES[target_cpu].queue.push(msg).expect("IPI mailbox full");
    trigger_interrupt(target_cpu); // Hardware IPI
}

// Receive IPI (called in interrupt handler on target CPU)
pub fn handle_ipi() {
    let cpu_id = current_cpu_id();
    while let Some(msg) = IPI_MAILBOXES[cpu_id].queue.pop() {
        match msg.kind {
            IpiKind::TlbShootdown { asid, vaddr } => {
                flush_tlb(asid, vaddr);
            }
            IpiKind::Reschedule => {
                current_cpu_state().scheduler.reschedule();
            }
            // ...
        }
    }
}
```

**Key insight**: Lock-free queues + ownership transfer = no contention.

---

## SMP Architecture (TASK-0012 Baseline)

### Per-CPU Ownership Model

```text
CPU 0                   CPU 1                   CPU 2
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│ PerCpuState     │    │ PerCpuState     │    │ PerCpuState     │
│ ├─ Scheduler    │    │ ├─ Scheduler    │    │ ├─ Scheduler    │
│ ├─ RunQueue     │    │ ├─ RunQueue     │    │ ├─ RunQueue     │
│ ├─ CurrentTask  │    │ ├─ CurrentTask  │    │ ├─ CurrentTask  │
│ └─ StackPool    │    │ └─ StackPool    │    │ └─ StackPool    │
└─────────────────┘    └─────────────────┘    └─────────────────┘
         │                      │                      │
         └──────────────────────┼──────────────────────┘
                                │
                         IPI Message Bus
                    (Lock-free mailboxes)
```

### Shared Immutable State

```text
                    ┌──────────────────────┐
                    │  VirtMachine (HAL)   │
                    │  ├─ UART base        │
                    │  ├─ Timer freq       │
                    │  └─ MMIO ranges      │
                    └──────────────────────┘
                              │
                    (Shared read-only, no locks)
                              │
         ┌────────────────────┼────────────────────┐
         │                    │                    │
      CPU 0                CPU 1                CPU 2
```

### Shared Mutable State (Minimal)

```text
                    ┌──────────────────────┐
                    │  GlobalIpcRouter     │
                    │  (Spin::Mutex)       │
                    │  └─ Message queues   │
                    └──────────────────────┘
                              │
                    (Lock required, keep critical
                     section short!)
                              │
         ┌────────────────────┼────────────────────┐
         │                    │                    │
      CPU 0                CPU 1                CPU 2
```

---

## Anti-Patterns (DON'T DO)

### ❌ 1. Shared Mutable State Without Synchronization

```rust
// BAD: Global mutable state without protection
static mut GLOBAL_COUNTER: usize = 0;

pub fn increment_counter() {
    unsafe {
        GLOBAL_COUNTER += 1; // DATA RACE!
    }
}
```

**Fix**: Use `AtomicUsize` or per-CPU counters.

---

### ❌ 2. Locks in Hot Paths

```rust
// BAD: Lock on every syscall
pub fn syscall_dispatch(num: usize, args: Args) -> isize {
    let _guard = SYSCALL_LOCK.lock(); // Contention!
    match num {
        // ...
    }
}
```

**Fix**: Make syscall handlers lock-free (per-CPU state).

---

### ❌ 3. Unbounded Queues

```rust
// BAD: Unbounded IPI queue (DoS vector)
pub struct IpiMailbox {
    queue: Vec<IpiMessage>, // Can grow indefinitely
}
```

**Fix**: Use bounded queues (`ArrayQueue<T, N>`).

Current kernel scheduler hardening follows the same rule: QoS runqueues are explicitly bounded and
enqueue saturation is handled via deterministic reject semantics instead of growth or unbounded retry.

---

### ❌ 4. Blocking in Interrupt Context

```rust
// BAD: Acquiring lock in IRQ handler
pub fn timer_irq_handler() {
    let mut scheduler = SCHEDULER.lock(); // Can deadlock!
    scheduler.tick();
}
```

**Fix**: Use lock-free atomics or defer work to non-IRQ context.

---

## Testing Strategy (Fearless Concurrency)

### 1. **Compile-Time Verification**

Rust's type system catches most concurrency bugs at compile time:

```rust
// This won't compile (Scheduler is !Send)
fn bad_example() {
    let scheduler = Scheduler::new();
    std::thread::spawn(move || {
        scheduler.schedule(); // ERROR: Scheduler is not Send
    });
}
```

### 2. **Loom (Concurrency Testing)**

For userspace libraries, use [Loom](https://github.com/tokio-rs/loom) to test lock-free algorithms:

```rust
#[cfg(test)]
mod tests {
    use loom::sync::atomic::{AtomicUsize, Ordering};
    use loom::thread;

    #[test]
    fn test_atomic_counter() {
        loom::model(|| {
            let counter = Arc::new(AtomicUsize::new(0));
            let c1 = counter.clone();
            let c2 = counter.clone();

            let t1 = thread::spawn(move || {
                c1.fetch_add(1, Ordering::Relaxed);
            });

            let t2 = thread::spawn(move || {
                c2.fetch_add(1, Ordering::Relaxed);
            });

            t1.join().unwrap();
            t2.join().unwrap();

            assert_eq!(counter.load(Ordering::Relaxed), 2);
        });
    }
}
```

### 3. **QEMU SMP Testing**

```bash
# Boot with 4 CPUs
qemu-system-riscv64 -smp 4 -kernel neuron.elf

# Expected markers:
# CPU 0: ready
# CPU 1: ready
# CPU 2: ready
# CPU 3: ready
# SMP: all CPUs online
```

---

## Performance Characteristics

### Lock-Free vs. Lock-Based

- **Lock-free (per-CPU)**: Low latency (no contention), high throughput, linear scalability
- **Spin locks (global)**: Medium latency/throughput, scalability plateaus around small core counts
- **Blocking locks**: High latency (context switches), low throughput, poor scalability

**NEURON strategy**: Maximize per-CPU ownership, minimize global locks.

---

## Migration Path (TASK-0011B → TASK-0012)

### Phase 1: TASK-0011B (Ownership Clarity)

- ✅ Document ownership model
- ✅ Add `Send`/`Sync` markers
- ✅ Newtype wrappers for handles

### Phase 2: TASK-0012 (SMP Implementation)

- ✅ Secondary hart bring-up + per-hart trap stack source
- ✅ Deterministic IPI selftests with anti-fake causal chain and counterfactual proofs
- ✅ Bounded work-stealing proof path + `test_reject_*` negatives
- ✅ TASK-0012B hardening bridge: bounded scheduler enqueue contract + explicit S_SOFT resched contract + guarded `tp->stack->BOOT` CPU-ID path
- 🔄 Full runtime `PerCpuScheduler` ownership model (post-v1/v1b follow-up hardening)

### Phase 3: TASK-0013 (QoS + Power)

- ✅ QoS ABI contract + authority-gated scheduling hints (v1)
- ✅ Deterministic timer coalescing service contract (v1)
- 🔄 Per-CPU QoS queues (SMP v2 follow-up)
- 🔄 CPU idle states (per-CPU follow-up)

---

## Related Documents

- `docs/architecture/vision.md` — Fearless concurrency as a core principle
- `tasks/TASK-0011B-kernel-rust-idioms-pre-smp.md` — Ownership prep work
- `tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md` — SMP v1 baseline (In Review)
- [Servo Parallel Architecture](https://github.com/servo/servo/wiki/Design) — Inspiration
- [Rust Atomics and Locks](https://marabos.nl/atomics/) — Concurrency patterns

---

## Summary

**Rust's ownership model enables NEURON to achieve:**

1. ✅ **Safe parallelism** without data races (compile-time guarantees)
2. ✅ **Lock-free hot paths** (per-CPU ownership)
3. ✅ **Explicit concurrency boundaries** (`Send`/`Sync` traits)
4. ✅ **Servo-style work stealing** (ownership transfer via message passing)

**Key insight**: Rust doesn't just prevent memory bugs—it prevents **concurrency bugs** at compile time.
This is NEURON's competitive advantage over C-based kernels (seL4, Zircon).
