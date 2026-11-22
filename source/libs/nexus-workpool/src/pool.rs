// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: THE process-wide compute pool (TASK-0276: one shared pool, never
//! a pool per subsystem) — a worker per CPU the process may run on (TASK-0365 P2: the
//! set its role was given, asked of the kernel; no program names a CPU or a count of
//! them), fence-coordinated (no busy spinning), deterministic partition → compute →
//! canonical reduce.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU markers (workpool determinism/bounded/sized ok); partition
//!   math, the sizing rule + the workers=1≡N equality matrix are host-tested in this crate.
//! INVARIANTS:
//!   - Workers are same-AS compute threads with EMPTY cap tables except the
//!     two fence caps transferred before resume (compute-only contract).
//!   - A worker is placed by the kernel inside the set it inherits from its process
//!     (a woken thread runs where it runs soonest); it pins itself nowhere.
//!   - The workers' stacks are one mapping of one object committed on first touch:
//!     sized by the worker count, one region of the address space, no page before use.
//!   - Exactly one job in flight (`run` is synchronous; a scoped job from
//!     `submit` is joined by `join_scoped` before its caller returns); a
//!     concurrent submit is REJECTED, never queued unbounded (TASK-0276 backpressure).
//!   - A scoped job (`bands.rs`) is joined without a deadline: the caller returns only
//!     after every worker checked in. A failed job signal poisons the pool (no seq skew).
//!   - Workers park in `fence_wait` (kernel-blocked) between jobs.
//!   - Determinism: chunk boundaries are pure (partition.rs); the caller's
//!     worker fn must write only its chunk's outputs; reduce = reading the
//!     outputs in index order after `run` returns.
//!
//! ADR: docs/adr/0046-deterministic-parallel-compute-workpool.md

use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use nexus_sched::CpuSet;

use crate::partition::chunk_bounds;

/// The pool's tables hold a worker per CPU a set can name — the one ceiling a machine has
/// (RFC-0118), never a count of workers.
const SLOTS: usize = CpuSet::BITS;

/// How many workers a process has: one per CPU it may run on — the set its role was given,
/// among the online ones — and one where the kernel answers nothing.
#[must_use]
pub const fn workers_for(may_run_on: CpuSet, online: CpuSet) -> usize {
    let cpus = may_run_on.intersection(online).count();
    if cpus == 0 {
        1
    } else {
        cpus
    }
}

// 64 KiB per worker: real workloads (SVG rasterization) overflowed 32 KiB. A worker that
// runs through its stack's floor writes into its neighbour's top; the canary below turns
// that into a loud, attributable error.
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
const WORKER_STACK_BYTES: usize = 64 * 1024;

/// Written at the LOW end of each worker stack before spawn; checked after
/// every run. A dead canary = the worker overflowed its stack.
/// (OS-only like the worker stacks themselves.)
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
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
    /// A previous run timed out with workers still executing: worker state is
    /// unknown, so the pool refuses all further jobs (callers use their
    /// inline fallback). Fail-closed — never hand a new job to workers that
    /// may still be writing the old one.
    Poisoned,
    /// A worker's stack canary is dead: it overflowed its stack and
    /// may have corrupted neighbouring memory. The pool poisons itself.
    StackOverflow,
    /// The workers' stacks could not be made or mapped during init.
    AbiStack,
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
    /// Read only by the OS-side worker entry loop; host builds never spawn
    /// workers, so the fields are write-only there.
    #[cfg_attr(
        not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")),
        allow(dead_code)
    )]
    job_fn: AtomicUsize,
    #[cfg_attr(
        not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")),
        allow(dead_code)
    )]
    job_ctx: AtomicUsize,
    #[cfg_attr(
        not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")),
        allow(dead_code)
    )]
    job_total: AtomicUsize,
    /// Monotonic job sequence; fence targets equal this value.
    /// (Like the job descriptor above: read on the OS-side worker/run paths.)
    #[cfg_attr(
        not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")),
        allow(dead_code)
    )]
    seq: AtomicU64,
    /// Workers that finished the current seq.
    done_count: AtomicUsize,
    /// Per-worker fence cap slots (in the WORKER's cap table), published
    /// before the worker is resumed.
    #[cfg_attr(
        not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")),
        allow(dead_code)
    )]
    worker_job_slot: [AtomicUsize; SLOTS],
    #[cfg_attr(
        not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")),
        allow(dead_code)
    )]
    worker_done_slot: [AtomicUsize; SLOTS],
    /// Parent-side fence cap slots.
    #[cfg_attr(
        not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")),
        allow(dead_code)
    )]
    parent_job_slot: AtomicUsize,
    #[cfg_attr(
        not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")),
        allow(dead_code)
    )]
    parent_done_slot: AtomicUsize,
    /// Diagnostics: workers that reached their entry loop.
    alive: [AtomicUsize; SLOTS],
    /// Diagnostics: fence_wait returns per worker.
    woke: [AtomicUsize; SLOTS],
    /// Where the workers' stacks are mapped (worker `i`: `WORKER_STACK_BYTES` from
    /// `stacks + i * WORKER_STACK_BYTES`).
    #[cfg_attr(
        not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")),
        allow(dead_code)
    )]
    stacks: AtomicUsize,
}

static SHARED: Shared = Shared {
    state: AtomicUsize::new(STATE_UNINIT),
    workers: AtomicUsize::new(0),
    job_fn: AtomicUsize::new(0),
    job_ctx: AtomicUsize::new(0),
    job_total: AtomicUsize::new(0),
    seq: AtomicU64::new(0),
    done_count: AtomicUsize::new(0),
    worker_job_slot: [const { AtomicUsize::new(0) }; SLOTS],
    worker_done_slot: [const { AtomicUsize::new(0) }; SLOTS],
    parent_job_slot: AtomicUsize::new(0),
    parent_done_slot: AtomicUsize::new(0),
    alive: [const { AtomicUsize::new(0) }; SLOTS],
    woke: [const { AtomicUsize::new(0) }; SLOTS],
    stacks: AtomicUsize::new(0),
};

/// Makes the workers' stacks: one object committed on first touch (a stack costs the pages
/// it reaches), mapped once at an address the kernel chooses — one region of the address
/// space however many workers the machine gives. Returns where.
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn map_stacks(workers: usize) -> Result<usize, PoolError> {
    use nexus_abi::page_flags::{READ, USER, VALID, WRITE};
    let len = workers * WORKER_STACK_BYTES;
    let object = nexus_abi::vmo_create_lazy(len).map_err(|_| PoolError::AbiStack)?;
    let mapped = nexus_abi::vm_map(object, 0, len, VALID | USER | READ | WRITE);
    // The mapping holds the object; the capability is not needed past it.
    let _ = nexus_abi::cap_close(object);
    mapped.map_err(|_| PoolError::AbiStack)
}

/// Worker `idx`'s stack.
///
/// # Safety
/// `base` is where [`map_stacks`] mapped at least `idx + 1` stacks; the caller is the one
/// user of worker `idx`'s range for the borrow.
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
unsafe fn stack_of(base: usize, idx: usize) -> &'static mut [u8] {
    // SAFETY: the range lies inside the mapping (the caller's contract), mapped read-write
    // for the rest of the program.
    unsafe {
        core::slice::from_raw_parts_mut(
            (base + idx * WORKER_STACK_BYTES) as *mut u8,
            WORKER_STACK_BYTES,
        )
    }
}

/// Worker loop: park in `fence_wait` until the next job seq, process our
/// deterministic chunk, last-one-out signals the done fence.
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
extern "C" fn worker_entry(idx: usize) {
    SHARED.alive[idx].store(1, Ordering::Release);
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
    let base = SHARED.stacks.load(Ordering::Acquire);
    (0..workers).all(|idx| {
        // SAFETY: the bottom 8 bytes of a worker's stack are reserved for the canary
        // (never valid stack data) and lie inside the mapping `init` made for `workers`.
        let floor = unsafe {
            core::ptr::read_volatile((base + idx * WORKER_STACK_BYTES) as *const [u8; 8])
        };
        floor == STACK_CANARY.to_le_bytes()
    })
}

/// Diagnostics for proofs: (alive workers, woke count, done_count).
pub fn selftest_debug() -> (usize, usize, u64) {
    let mut alive = 0;
    let mut woke = 0;
    for idx in 0..SLOTS {
        alive += SHARED.alive[idx].load(Ordering::Acquire);
        woke += SHARED.woke[idx].load(Ordering::Acquire);
    }
    (alive, woke, SHARED.done_count.load(Ordering::Acquire) as u64)
}

/// The pool's workers: as many as CPUs the process may run on; 0 before [`init`].
#[must_use]
pub fn workers() -> usize {
    match SHARED.state.load(Ordering::Acquire) {
        STATE_UNINIT | STATE_INIT_IN_PROGRESS => 0,
        _ => SHARED.workers.load(Ordering::Acquire),
    }
}

/// Diagnostic: does the parent's own job-fence cap observe its own signal?
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
pub fn selftest_probe_job_selfsignal() -> bool {
    let job_slot = SHARED.parent_job_slot.load(Ordering::Acquire) as u32;
    let seq = SHARED.seq.load(Ordering::Acquire);
    let deadline = nexus_abi::nsec().unwrap_or(0).saturating_add(100_000_000);
    nexus_abi::fence_wait(job_slot, seq, deadline).is_ok()
}

/// Initializes THE process pool: a compute thread per CPU the process may run on
/// ([`workers_for`] of its own set and the online one — the caller names no number).
/// Returns how many. Call once; further calls are rejected.
pub fn init() -> Result<usize, PoolError> {
    if SHARED
        .state
        .compare_exchange(STATE_UNINIT, STATE_INIT_IN_PROGRESS, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(PoolError::AlreadyInitialized);
    }
    // The kernel's two answers; a host build has no kernel to ask and gets the one worker.
    #[cfg(nexus_env = "os")]
    let (own, online) = {
        let online = nexus_abi::sched::online().unwrap_or(CpuSet::EMPTY);
        (nexus_abi::sched::get_affinity().unwrap_or(online), online)
    };
    #[cfg(not(nexus_env = "os"))]
    let (own, online) = (CpuSet::EMPTY, CpuSet::EMPTY);
    let workers = workers_for(own, online);

    #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
    {
        let result = (|| -> Result<(), PoolError> {
            let stacks = map_stacks(workers)?;
            SHARED.stacks.store(stacks, Ordering::Release);
            let job_fence = nexus_abi::fence_create().map_err(|_| PoolError::AbiFence)?;
            let done_fence = nexus_abi::fence_create().map_err(|_| PoolError::AbiFence)?;
            SHARED.parent_job_slot.store(job_fence as usize, Ordering::Release);
            SHARED.parent_done_slot.store(done_fence as usize, Ordering::Release);
            SHARED.workers.store(workers, Ordering::Release);

            for idx in 0..workers {
                // SAFETY: `stacks` maps `workers` stacks; worker `idx` gets exactly its own.
                let stack = unsafe { stack_of(stacks, idx) };
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
                nexus_abi::task_resume(pid).map_err(|_| PoolError::AbiResume)?;
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                SHARED.state.store(STATE_READY, Ordering::Release);
                Ok(workers)
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
        Ok(workers)
    }
}

/// Takes the pool for one job (`READY → RUNNING`), or says why not — nothing changes then.
fn take_pool() -> Result<(), PoolError> {
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
    Ok(())
}

/// Hands the taken pool a job and wakes the workers; returns the job's sequence number.
///
/// A failed signal POISONS the pool: the sequence number has advanced while the fence has not,
/// so the next signal would wake the workers for two jobs at once — they would run one job twice
/// and check in twice, and a scoped job ([`join_scoped`]) would be joined while a worker still
/// paints. `Err(Abi)` is then still "nothing started": no worker saw this job.
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn post(total: usize, f: JobFn, ctx: *mut u8) -> Result<u64, PoolError> {
    SHARED.job_fn.store(f as usize, Ordering::Release);
    SHARED.job_ctx.store(ctx as usize, Ordering::Release);
    SHARED.job_total.store(total, Ordering::Release);
    SHARED.done_count.store(0, Ordering::Release);
    let seq = SHARED.seq.fetch_add(1, Ordering::AcqRel) + 1;
    let job_slot = SHARED.parent_job_slot.load(Ordering::Acquire) as u32;
    if nexus_abi::fence_signal(job_slot, seq).is_err() {
        SHARED.state.store(STATE_POISONED, Ordering::Release);
        return Err(PoolError::Abi);
    }
    Ok(seq)
}

/// Starts a scoped job (the band entry, `bands.rs`): takes the pool and wakes the workers.
/// Returns `(workers, seq)`. `Err`: nothing was published — no worker will touch `ctx`.
/// The caller MUST [`join_scoped`] the returned `seq` before `ctx` goes away.
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
pub(crate) fn submit(total: usize, f: JobFn, ctx: *mut u8) -> Result<(usize, u64), PoolError> {
    take_pool()?;
    let seq = post(total, f, ctx)?;
    Ok((SHARED.workers.load(Ordering::Acquire), seq))
}

/// Joins scoped job `seq` WITHOUT a deadline (ADR-0046: a scoped entry joins before it
/// returns): returns only once every worker has checked in — none will touch the job again, and
/// the Acquire load of the check-ins orders their writes before the caller's next step (a cache
/// clean among them). The done fence is how the caller sleeps; a fence that fails degrades to a
/// yield loop on the same count and poisons the pool afterwards — it never returns early.
/// Returns whether every worker's stack canary survived; a dead one poisons the pool.
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
pub(crate) fn join_scoped(seq: u64) -> bool {
    let workers = SHARED.workers.load(Ordering::Acquire);
    let done_slot = SHARED.parent_done_slot.load(Ordering::Acquire) as u32;
    let mut fence = true;
    while SHARED.done_count.load(Ordering::Acquire) < workers {
        if fence && nexus_abi::fence_wait(done_slot, seq, 0).is_err() {
            fence = false;
        }
        if !fence {
            let _ = nexus_abi::yield_();
        }
    }
    if !canaries_intact() {
        SHARED.state.store(STATE_POISONED, Ordering::Release);
        return false;
    }
    SHARED.state.store(if fence { STATE_READY } else { STATE_POISONED }, Ordering::Release);
    true
}

/// Runs one job synchronously: splits `[0, total)` into the pool's fixed
/// deterministic chunks, lets every worker process its chunk, returns when
/// all are done (or fails loudly after `deadline_ns`, never hangs). The
/// canonical reduce is the caller reading its outputs in index order after
/// this returns.
pub fn run(total: usize, f: JobFn, ctx: *mut u8, deadline_ns: u64) -> Result<(), PoolError> {
    take_pool()?;

    #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
    {
        let seq = post(total, f, ctx)?;
        let done_slot = SHARED.parent_done_slot.load(Ordering::Acquire) as u32;
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
