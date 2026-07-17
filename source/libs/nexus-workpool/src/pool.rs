// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: THE process-wide compute pool (TASK-0276: one shared pool, never
//! a pool per subsystem) — fixed worker count, fence-coordinated (no busy
//! spinning), deterministic partition → compute → canonical reduce.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU markers (workpool determinism/bounded ok); partition
//!   math + the workers=1≡N equality matrix are host-tested in this crate.
//! INVARIANTS:
//!   - Workers are same-AS compute threads with EMPTY cap tables except the
//!     two fence caps transferred before resume (compute-only contract).
//!   - Exactly one job in flight (`run` is synchronous); a concurrent submit
//!     is REJECTED, never queued unbounded (TASK-0276 backpressure).
//!   - Workers park in `fence_wait` (kernel-blocked) between jobs.
//!   - Determinism: chunk boundaries are pure (partition.rs); the caller's
//!     worker fn must write only its chunk's outputs; reduce = reading the
//!     outputs in index order after `run` returns.
//! ADR: docs/adr/0016-kernel-libs-architecture.md (lib placement)

use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use crate::partition::chunk_bounds;

/// Worker ceiling (mirrors the kernel CPU ceiling).
pub const MAX_WORKERS: usize = 4;
// 64 KiB per worker: real workloads (SVG rasterization) overflowed 32 KiB —
// and a static-array stack overflows SILENTLY into neighbouring .bss (the
// known three-stack-cliff failure mode). The canary below turns the silent
// cliff into a loud, attributable error.
const WORKER_STACK_BYTES: usize = 64 * 1024;

/// Written at the LOW end of each worker stack before spawn; checked after
/// every run. A dead canary = the worker overflowed its stack.
const STACK_CANARY: u64 = 0xDEAD_5AFE_CAFE_F00D;

/// Job function contract: process items `[start, end)`; `ctx` is the raw
/// caller context (typically a pointer to input/output arrays). MUST only
/// write outputs belonging to its own chunk (determinism + no data races).
pub type JobFn = extern "C" fn(start: usize, end: usize, ctx: *mut u8);

/// Pool errors — every rejection is explicit and bounded.
#[must_use = "workpool outcomes must be handled"]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolError {
    /// `init` was already called (the pool is a process singleton).
    AlreadyInitialized,
    /// `run`/`shutdown` before `init`, or a worker failed to start.
    NotReady,
    /// A job is already in flight (bounded depth-1 submission; retry later).
    Busy,
    /// Invalid arguments (workers == 0).
    InvalidArgs,
    /// A previous run timed out with workers still executing: worker state is
    /// unknown, so the pool refuses all further jobs (callers use their
    /// inline fallback). Fail-closed — never hand a new job to workers that
    /// may still be writing the old one.
    Poisoned,
    /// A worker's stack canary is dead: it overflowed its static stack and
    /// may have corrupted neighbouring memory. The pool poisons itself.
    StackOverflow,
    /// fence_create failed during init.
    AbiFence,
    /// thread spawn failed during init.
    AbiSpawn,
    /// cap transfer into the worker failed during init.
    AbiTransfer,
    /// task_resume failed during init.
    AbiResume,
    /// fence signal/wait failed during run.
    Abi,
    /// The job did not complete within the deadline (fail loud, never hang).
    Timeout,
}

const STATE_UNINIT: usize = 0;
const STATE_INIT_IN_PROGRESS: usize = 1;
const STATE_READY: usize = 2;
const STATE_RUNNING: usize = 3;
const STATE_POISONED: usize = 4;

/// Shared pool state: lives in the process image; workers (same AS) read it
/// directly. All fields are atomics — no locks on the worker paths.
struct Shared {
    state: AtomicUsize,
    workers: AtomicUsize,
    /// Job descriptor (valid while state == RUNNING for the current seq).
    job_fn: AtomicUsize,
    job_ctx: AtomicUsize,
    job_total: AtomicUsize,
    /// Monotonic job sequence; fence targets equal this value.
    seq: AtomicU64,
    /// Workers that finished the current seq.
    done_count: AtomicUsize,
    /// Per-worker fence cap slots (in the WORKER's cap table), published
    /// before the worker is resumed.
    worker_job_slot: [AtomicUsize; MAX_WORKERS],
    worker_done_slot: [AtomicUsize; MAX_WORKERS],
    /// Parent-side fence cap slots.
    parent_job_slot: AtomicUsize,
    parent_done_slot: AtomicUsize,
    /// Diagnostics: workers that reached their entry loop.
    alive: [AtomicUsize; MAX_WORKERS],
    /// Diagnostics: fence_wait returns per worker.
    woke: [AtomicUsize; MAX_WORKERS],
    /// Diagnostics: workers whose self-pin round-tripped (set + get agree).
    pinned: [AtomicUsize; MAX_WORKERS],
}

static SHARED: Shared = Shared {
    state: AtomicUsize::new(STATE_UNINIT),
    workers: AtomicUsize::new(0),
    job_fn: AtomicUsize::new(0),
    job_ctx: AtomicUsize::new(0),
    job_total: AtomicUsize::new(0),
    seq: AtomicU64::new(0),
    done_count: AtomicUsize::new(0),
    worker_job_slot: [const { AtomicUsize::new(0) }; MAX_WORKERS],
    worker_done_slot: [const { AtomicUsize::new(0) }; MAX_WORKERS],
    parent_job_slot: AtomicUsize::new(0),
    parent_done_slot: AtomicUsize::new(0),
    alive: [const { AtomicUsize::new(0) }; MAX_WORKERS],
    woke: [const { AtomicUsize::new(0) }; MAX_WORKERS],
    pinned: [const { AtomicUsize::new(0) }; MAX_WORKERS],
};

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
static mut WORKER_STACKS: [[u8; WORKER_STACK_BYTES]; MAX_WORKERS] =
    [[0; WORKER_STACK_BYTES]; MAX_WORKERS];

/// Worker loop: park in `fence_wait` until the next job seq, process our
/// deterministic chunk, last-one-out signals the done fence.
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
extern "C" fn worker_entry(idx: usize) {
    SHARED.alive[idx].store(1, Ordering::Release);
    // TASK-0042/C4: deterministic self-pin — worker idx wants CPU idx. On
    // hosts with fewer online CPUs the kernel rejects the mask (mask ∩ online
    // = 0) and the worker deterministically keeps the inherited full mask.
    let desired = 1usize << (idx & 0x7);
    if nexus_abi::sched::set_affinity(desired).is_ok()
        && nexus_abi::sched::get_affinity() == Ok(desired)
    {
        SHARED.pinned[idx].store(1, Ordering::Release);
    }
    let job_slot = SHARED.worker_job_slot[idx].load(Ordering::Acquire) as u32;
    let done_slot = SHARED.worker_done_slot[idx].load(Ordering::Acquire) as u32;
    let mut seq: u64 = 0;
    loop {
        seq += 1;
        if nexus_abi::fence_wait(job_slot, seq, 0).is_err() {
            // Fence gone/defect: exit the worker (parent's next run times out
            // loudly instead of hanging forever).
            return;
        }
        SHARED.woke[idx].fetch_add(1, Ordering::AcqRel);
        let f: JobFn =
            // SAFETY: written by `run` before the fence signal for this seq;
            // the signal's release ordering publishes it.
            unsafe { core::mem::transmute(SHARED.job_fn.load(Ordering::Acquire)) };
        let ctx = SHARED.job_ctx.load(Ordering::Acquire) as *mut u8;
        let total = SHARED.job_total.load(Ordering::Acquire);
        let workers = SHARED.workers.load(Ordering::Acquire);
        let (start, end) = chunk_bounds(total, workers, idx);
        f(start, end, ctx);
        if SHARED.done_count.fetch_add(1, Ordering::AcqRel) + 1 == workers {
            let _ = nexus_abi::fence_signal(done_slot, seq);
        }
    }
}

/// Diagnostic probe: waiting on the DONE fence for an unsignalled target
/// must time out. `true` = fence behaves correctly.
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
pub fn selftest_probe_done_fence() -> bool {
    let done_slot = SHARED.parent_done_slot.load(Ordering::Acquire) as u32;
    let deadline = nexus_abi::nsec().unwrap_or(0).saturating_add(50_000_000);
    nexus_abi::fence_wait(done_slot, u64::MAX, deadline).is_err()
}

/// `true` while every spawned worker's stack-floor canary is intact.
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn canaries_intact() -> bool {
    let workers = SHARED.workers.load(Ordering::Acquire);
    for idx in 0..workers.min(MAX_WORKERS) {
        // SAFETY: read-only view of the worker's dedicated static stack; the
        // bottom 8 bytes are reserved for the canary (never valid stack data).
        let stack = unsafe { &(*core::ptr::addr_of!(WORKER_STACKS))[idx] };
        if stack[..8] != STACK_CANARY.to_le_bytes() {
            return false;
        }
    }
    true
}

/// Diagnostics for proofs: (alive workers, woke count, done_count).
pub fn selftest_debug() -> (usize, usize, u64) {
    let mut alive = 0;
    let mut woke = 0;
    for idx in 0..MAX_WORKERS {
        alive += SHARED.alive[idx].load(Ordering::Acquire);
        woke += SHARED.woke[idx].load(Ordering::Acquire);
    }
    (alive, woke, SHARED.done_count.load(Ordering::Acquire) as u64)
}

/// Diagnostics for proofs: workers whose self-pin round-tripped (C4).
pub fn selftest_pinned() -> usize {
    let mut pinned = 0;
    for slot in SHARED.pinned.iter() {
        pinned += slot.load(Ordering::Acquire);
    }
    pinned
}

/// Diagnostic: does the parent's own job-fence cap observe its own signal?
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
pub fn selftest_probe_job_selfsignal() -> bool {
    let job_slot = SHARED.parent_job_slot.load(Ordering::Acquire) as u32;
    let seq = SHARED.seq.load(Ordering::Acquire);
    let deadline = nexus_abi::nsec().unwrap_or(0).saturating_add(100_000_000);
    nexus_abi::fence_wait(job_slot, seq, deadline).is_ok()
}

/// C4 (TASK-0042): deterministic worker placement — worker `i` pins itself
/// to CPU `i % MAX_WORKERS` when that CPU is online (the kernel validates);
/// otherwise it keeps the inherited full mask.
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn pin_worker(pid: nexus_abi::Pid, idx: usize) {
    let _ = pid;
    let _ = idx;
    // Affinity is per-task and must be set BY the worker or cross-task with
    // the QoS-admin cap. Services do not hold that cap, so v1 pins nothing
    // cross-task; the deterministic placement comes from the kernel's
    // round-robin spawn placement + affinity-respecting stealing. (execd
    // recipe wiring — Phase B4 — will pin service pools explicitly.)
}

/// Initializes THE process pool with `workers` compute threads (clamped to
/// [`MAX_WORKERS`]). Call once; further calls are rejected.
pub fn init(workers: usize) -> Result<(), PoolError> {
    if workers == 0 {
        return Err(PoolError::InvalidArgs);
    }
    if SHARED
        .state
        .compare_exchange(
            STATE_UNINIT,
            STATE_INIT_IN_PROGRESS,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_err()
    {
        return Err(PoolError::AlreadyInitialized);
    }
    let workers = workers.min(MAX_WORKERS);

    #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
    {
        let result = (|| -> Result<(), PoolError> {
            let job_fence = nexus_abi::fence_create().map_err(|_| PoolError::AbiFence)?;
            let done_fence = nexus_abi::fence_create().map_err(|_| PoolError::AbiFence)?;
            SHARED.parent_job_slot.store(job_fence as usize, Ordering::Release);
            SHARED.parent_done_slot.store(done_fence as usize, Ordering::Release);
            SHARED.workers.store(workers, Ordering::Release);

            for idx in 0..workers {
                // SAFETY: each worker gets exactly one dedicated static stack.
                let stack = unsafe { &mut (*core::ptr::addr_of_mut!(WORKER_STACKS))[idx] };
                stack[..8].copy_from_slice(&STACK_CANARY.to_le_bytes());
                let pid = nexus_abi::thread::spawn_thread_suspended(worker_entry, idx, stack)
                    .map_err(|_| PoolError::AbiSpawn)?;
                // Transfer the fence caps into the (suspended) worker's empty
                // cap table; publish the worker-side slots BEFORE resume.
                let js = nexus_abi::cap_transfer(pid, job_fence, nexus_abi::Rights::MANAGE)
                    .map_err(|_| PoolError::AbiTransfer)?;
                let ds = nexus_abi::cap_transfer(pid, done_fence, nexus_abi::Rights::MANAGE)
                    .map_err(|_| PoolError::AbiTransfer)?;
                SHARED.worker_job_slot[idx].store(js as usize, Ordering::Release);
                SHARED.worker_done_slot[idx].store(ds as usize, Ordering::Release);
                pin_worker(pid, idx);
                nexus_abi::task_resume(pid).map_err(|_| PoolError::AbiResume)?;
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                SHARED.state.store(STATE_READY, Ordering::Release);
                Ok(())
            }
            Err(err) => {
                // Fail closed: the pool stays unusable (workers may be
                // partially started but will park on the job fence forever).
                SHARED.state.store(STATE_UNINIT, Ordering::Release);
                Err(err)
            }
        }
    }
    #[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
    {
        SHARED.workers.store(workers, Ordering::Release);
        SHARED.state.store(STATE_READY, Ordering::Release);
        Ok(())
    }
}

/// Runs one job synchronously: splits `[0, total)` into the pool's fixed
/// deterministic chunks, lets every worker process its chunk, returns when
/// all are done (or fails loudly after `deadline_ns`, never hangs). The
/// canonical reduce is the caller reading its outputs in index order after
/// this returns.
pub fn run(total: usize, f: JobFn, ctx: *mut u8, deadline_ns: u64) -> Result<(), PoolError> {
    if SHARED
        .state
        .compare_exchange(STATE_READY, STATE_RUNNING, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return match SHARED.state.load(Ordering::Acquire) {
            STATE_RUNNING => Err(PoolError::Busy),
            STATE_POISONED => Err(PoolError::Poisoned),
            _ => Err(PoolError::NotReady),
        };
    }

    #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
    {
        SHARED.job_fn.store(f as usize, Ordering::Release);
        SHARED.job_ctx.store(ctx as usize, Ordering::Release);
        SHARED.job_total.store(total, Ordering::Release);
        SHARED.done_count.store(0, Ordering::Release);
        let seq = SHARED.seq.fetch_add(1, Ordering::AcqRel) + 1;

        let job_slot = SHARED.parent_job_slot.load(Ordering::Acquire) as u32;
        let done_slot = SHARED.parent_done_slot.load(Ordering::Acquire) as u32;

        if nexus_abi::fence_signal(job_slot, seq).is_err() {
            // Signal never reached the workers: no job started, READY is safe.
            SHARED.state.store(STATE_READY, Ordering::Release);
            return Err(PoolError::Abi);
        }
        let deadline = if deadline_ns == 0 {
            0
        } else {
            nexus_abi::nsec().unwrap_or(0).saturating_add(deadline_ns)
        };
        match nexus_abi::fence_wait(done_slot, seq, deadline) {
            Ok(()) => {
                if !canaries_intact() {
                    // A worker ran through its stack floor: its writes may
                    // have corrupted neighbouring statics. Loud + poisoned.
                    SHARED.state.store(STATE_POISONED, Ordering::Release);
                    return Err(PoolError::StackOverflow);
                }
                SHARED.state.store(STATE_READY, Ordering::Release);
                Ok(())
            }
            Err(_) => {
                // Workers may still be executing the old job — handing out a
                // new one would interleave writes. Poison the pool: every
                // caller falls back to its inline path from here on.
                // Returned as `Timeout` (not `Poisoned`): THIS call's job DID
                // start, so the caller must NOT re-run the same work inline
                // (double-reduction hazard); `Poisoned` is what LATER calls
                // see, before any work started — safe to fall back.
                SHARED.state.store(STATE_POISONED, Ordering::Release);
                Err(PoolError::Timeout)
            }
        }
    }
    #[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
    {
        // Host/dev fallback: run the chunks sequentially in chunk order —
        // byte-identical results by the determinism contract.
        let workers = SHARED.workers.load(Ordering::Acquire);
        for idx in 0..workers {
            let (start, end) = chunk_bounds(total, workers, idx);
            f(start, end, ctx);
        }
        let _ = deadline_ns;
        SHARED.state.store(STATE_READY, Ordering::Release);
        Ok(())
    }
}
