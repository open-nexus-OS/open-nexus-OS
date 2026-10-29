// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the kernel's one memory-pressure sink (TASK-0333 6e-0, RFC-0100 §3, ADR-0085).
//! The frame pool reports its free frames after every allocation and free (`observe`); the
//! tracker (`crate::pressure`, pure) turns them into a level, and a level change is left
//! pending here. The ONE service that bound an endpoint (`SYSCALL_MEM_PRESSURE_BIND`, 67 — the
//! shape of `child_exit_bind`) receives one `OP_MEMORY_PRESSURE {level, free, total}` frame per
//! change, delivered where the kernel already delivers its other events — the syscall
//! epilogue, the user fault's fill, the timer tick and the idle loop — never from inside the
//! pool's lock. A bind delivers the level of the moment, so the binder never has to ask.
//!
//! One binder: the first service to bind holds the sink until its endpoint dies; another
//! service's bind is refused (`PermissionDenied`), the holder may re-bind or clear. The
//! lifecycle authority of memory is `memd` (ADR-0086); init starts it before any app runs. The
//! holder is the swap stage's pager too (6f-2, ADR-0088): the same sink carries its page-in
//! requests and dropped slots (`mm::swap`, delivered after the level by the same dispatch), and
//! the swap syscalls answer only the holder.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional (TASK-0333 6e-0)
//! API_STABILITY: Internal
//! TEST_COVERAGE: the rule in `crate::pressure` (host); `KSELFTEST: mem pressure notify ok (…)`
//!   proves bind → frame, the injected level, the second binder's refusal and the clear on
//!   every QEMU boot; `KSELFTEST: mem watermarks ok (…)` the watermarks of the booted pool
//! INVARIANTS: nothing here allocates; the pool lock is never held across a send; a pending
//!   level is one byte (the latest wins — a service that missed a step reads `free` and
//!   `total` in the frame it does get)

use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};

use crate::ipc::{self, EndpointId};
use crate::pressure::{self, Level, Tracker, Watermarks};
use crate::sched::Scheduler;
use crate::task;

/// The rule over the booted pool; `None` until the pool exists.
static TRACKER: spin::Mutex<Option<Tracker>> = spin::Mutex::new(None);

/// The level to deliver next, plus one; 0 = nothing pending.
static PENDING: AtomicU8 = AtomicU8::new(0);

/// The bound endpoint (0 = none) and the pid that bound it, plus one (0 = none).
static BINDING: AtomicU32 = AtomicU32::new(0);
static BINDER: AtomicU32 = AtomicU32::new(0);

/// Why a bind was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindError {
    /// Another process holds the sink.
    Held,
}

/// The pool is built: the watermarks from what each bank has free now, the level from the sum.
pub fn init(boot_free_per_bank: impl IntoIterator<Item = u64>, free: u64) {
    let marks = Watermarks::from_banks(boot_free_per_bank);
    *TRACKER.lock() = Some(Tracker::new(marks, free));
}

/// The pool's free frames changed. Leaf: called with the pool lock dropped, takes only its own.
pub fn observe(free: u64) {
    let mut guard = TRACKER.lock();
    let Some(tracker) = guard.as_mut() else { return };
    if let Some(level) = tracker.observe(free) {
        PENDING.store(level as u8 + 1, Ordering::Release);
    }
}

/// The level now, and the watermarks (`None` before the pool exists).
pub fn state() -> Option<(Level, Watermarks)> {
    TRACKER.lock().map(|t| (t.level(), t.watermarks()))
}

/// Binds `endpoint` as the sink for `pid`; the level of the moment is delivered next. Refused
/// while another process holds the sink.
pub fn bind(endpoint: EndpointId, pid: task::Pid) -> Result<(), BindError> {
    let holder = BINDER.load(Ordering::Acquire);
    if holder != 0 && holder != pid.as_raw() + 1 {
        return Err(BindError::Held);
    }
    BINDING.store(endpoint, Ordering::Release);
    BINDER.store(pid.as_raw() + 1, Ordering::Release);
    let level = state().map_or(Level::Normal, |(level, _)| level);
    PENDING.store(level as u8 + 1, Ordering::Release);
    Ok(())
}

/// Clears the sink `pid` holds (a stranger's clear is refused).
pub fn unbind(pid: task::Pid) -> Result<(), BindError> {
    let holder = BINDER.load(Ordering::Acquire);
    if holder != 0 && holder != pid.as_raw() + 1 {
        return Err(BindError::Held);
    }
    BINDING.store(0, Ordering::Release);
    BINDER.store(0, Ordering::Release);
    PENDING.store(0, Ordering::Release);
    Ok(())
}

/// The endpoint died (whichever path killed it): the sink is free again.
pub fn forget_endpoint(endpoint: EndpointId) {
    if BINDING.load(Ordering::Acquire) == endpoint {
        BINDING.store(0, Ordering::Release);
        BINDER.store(0, Ordering::Release);
        PENDING.store(0, Ordering::Release);
    }
}

/// The bound endpoint, if any.
pub fn binding() -> Option<EndpointId> {
    match BINDING.load(Ordering::Acquire) {
        0 => None,
        endpoint => Some(endpoint),
    }
}

/// The process holding the sink — the pager of the swap stage (ADR-0088), if any.
pub fn holder() -> Option<task::Pid> {
    BINDER.load(Ordering::Acquire).checked_sub(1).map(task::Pid::from_raw)
}

/// Whether a level or a swap notice waits for a sink.
pub fn pending() -> bool {
    PENDING.load(Ordering::Acquire) != 0 || super::swap::pending()
}

/// The boot proof's injector: leave `level` pending as a change would.
pub fn selftest_inject(level: Level) {
    PENDING.store(level as u8 + 1, Ordering::Release);
}

/// One frame (its first byte the op) into the sink `endpoint`, its waiter woken. `false`: the
/// sink is full (or gone) — the caller keeps what it could not send.
pub(super) fn send_frame(
    router: &mut ipc::Router,
    tasks: &mut task::TaskTable,
    scheduler: &mut Scheduler,
    endpoint: EndpointId,
    payload: &[u8],
) -> bool {
    let op = payload.first().copied().unwrap_or(0);
    let header =
        ipc::header::MessageHeader::new(0, endpoint, u16::from(op), 0, payload.len() as u32);
    let msg = ipc::Message::new(header, ipc::Payload::from_slice(payload), None);
    if router.send(endpoint, msg).is_err() {
        return false;
    }
    if let Ok(Some(waiter)) = router.pop_recv_waiter(endpoint) {
        let _ = tasks.wake(task::Pid::from_raw(waiter), scheduler);
    }
    true
}

/// Delivers the pending level and the swap stage's notices (ADR-0088) to the sink and wakes its
/// waiter. Caller contract: exclusive borrows (a syscall, a user trap, the idle loop under the
/// kernel lock), no pool lock held.
pub fn dispatch(router: &mut ipc::Router, tasks: &mut task::TaskTable, scheduler: &mut Scheduler) {
    if PENDING.load(Ordering::Acquire) != 0 {
        deliver_level(router, tasks, scheduler);
    }
    // After the level: a purge the level ran may have dropped swapped pages.
    super::swap::deliver(router, tasks, scheduler);
}

/// The pending level, delivered.
fn deliver_level(router: &mut ipc::Router, tasks: &mut task::TaskTable, scheduler: &mut Scheduler) {
    let raw = PENDING.swap(0, Ordering::AcqRel);
    let Some(level) = raw.checked_sub(1).and_then(Level::from_raw) else { return };
    // RFC-0100 §4: at `warn` and above the objects their owners unlocked go first — here, where
    // the change is delivered and no pool lock is held — so the frame carries what came back.
    // With or without a listener: the purge is the kernel's own answer to its level; a listener
    // that binds later is told the level of that moment by its bind.
    if level >= Level::Warn {
        let purged = super::vmo::purge::purge_unlocked();
        if purged > 0 {
            log_info!(target: "mm", "MM: pressure purge level={} pages={}", level.name(), purged);
        }
    }
    let Some(endpoint) = binding() else { return };
    let (free, total) =
        super::frame_pool::stats().map_or((0, 0), |s| (s.free as u64, s.total as u64));
    let payload = pressure::encode(level, free, total);
    if !send_frame(router, tasks, scheduler, endpoint, &payload) {
        // The sink is full: the level is not lost, the next change carries the truth of its
        // moment, and `mm_stats` answers the level at any time.
        log_debug!(target: "mm", "MM: pressure notify dropped level={}", level.name());
    }
}
