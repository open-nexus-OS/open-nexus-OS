// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Kernel sync-object syscalls split out of the former single-file
//! api.rs: timers (sys_timer_*), IRQ binding (sys_irq_*), waitsets
//! (sys_waitset_*) and timeline fences (sys_fence_*), incl. the *_id_from_cap
//! ownership checks and error mapping (RFC-0033).
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: neuron host tests + QEMU marker gates (just test-os / ci-os-smp)
//! ADR: docs/adr/0016-kernel-libs-architecture.md

use super::*;

#[inline]
pub(super) fn map_timer_error(err: crate::timer::TimerError) -> Error {
    match err {
        crate::timer::TimerError::ResourceExhausted => Error::Capability(CapError::NoSpace),
        crate::timer::TimerError::InvalidHandle => Error::Capability(CapError::InvalidSlot),
        crate::timer::TimerError::AlreadyArmed => Error::Capability(CapError::PermissionDenied),
    }
}

#[inline]
pub(super) fn timer_id_from_cap(
    ctx: &mut Context<'_>,
    slot: usize,
) -> Result<crate::timer::TimerId, Error> {
    let cap = ctx.tasks.current_caps_mut().get(slot)?;
    if !cap.rights.contains(Rights::MANAGE) {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    match cap.kind {
        CapabilityKind::Timer(id) => {
            let timer_id = crate::timer::TimerId(id);
            if !ctx.hart_timers.owned_by(timer_id, ctx.tasks.current_pid().as_raw()) {
                return Err(Error::Capability(CapError::PermissionDenied));
            }
            Ok(timer_id)
        }
        _ => Err(Error::Capability(CapError::InvalidSlot)),
    }
}

pub(super) fn sys_timer_create(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let notify_slot = args.get(0);
    let interval_ns = args.get(1) as u64;
    let notify_cap = ctx.tasks.current_caps_mut().get(notify_slot)?;
    let notify_ep = match notify_cap.kind {
        CapabilityKind::Endpoint(id) => id,
        _ => return Err(Error::Capability(CapError::InvalidSlot)),
    };
    let timer_id = ctx
        .hart_timers
        .alloc(ctx.tasks.current_pid().as_raw(), notify_ep, interval_ns)
        .map_err(map_timer_error)?;
    let timer_cap = Capability { kind: CapabilityKind::Timer(timer_id.0), rights: Rights::MANAGE };
    match ctx.tasks.current_caps_mut().allocate(timer_cap) {
        Ok(slot) => Ok(slot),
        Err(err) => {
            let _ = ctx.hart_timers.free(timer_id);
            Err(Error::Capability(err))
        }
    }
}

pub(super) fn sys_timer_set(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let slot = args.get(0);
    let deadline_ns = args.get(1) as u64;
    let timer_id = timer_id_from_cap(ctx, slot)?;
    ctx.hart_timers.arm(timer_id, deadline_ns).map_err(map_timer_error)?;
    ctx.timer.set_wakeup(deadline_ns);
    Ok(0)
}

pub(super) fn sys_timer_cancel(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let slot = args.get(0);
    let timer_id = timer_id_from_cap(ctx, slot)?;
    ctx.hart_timers.disarm(timer_id).map_err(map_timer_error)?;
    Ok(0)
}

/// Binds an external interrupt (PLIC source) to an endpoint the caller owns, so
/// the kernel routes that device IRQ to the driver's endpoint and wakes it. The
/// caller must hold an Endpoint capability for the receiving endpoint (proves it
/// is the intended driver). Args: (irq_source_id, endpoint_cap_slot).
pub(super) fn sys_irq_bind(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let irq_raw = args.get(0) as u32;
    let endpoint_slot = args.get(1);
    let irq =
        crate::hal::plic::IrqId::new(irq_raw).ok_or(Error::Capability(CapError::InvalidSlot))?;
    let endpoint = match ctx.tasks.current_caps_mut().get(endpoint_slot)?.kind {
        CapabilityKind::Endpoint(id) => id,
        _ => return Err(Error::Capability(CapError::InvalidSlot)),
    };
    crate::irq::bind(irq, endpoint);
    Ok(0)
}

/// Acknowledges a delivered IRQ so the PLIC re-arms it. The driver calls this
/// after it has drained the device (cleared the interrupt condition). Args:
/// (irq_source_id).
pub(super) fn sys_irq_complete(_ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let irq_raw = args.get(0) as u32;
    let irq =
        crate::hal::plic::IrqId::new(irq_raw).ok_or(Error::Capability(CapError::InvalidSlot))?;
    crate::irq::complete(irq);
    Ok(0)
}

#[inline]
pub(super) fn map_waitset_error(err: crate::waitset::WaitsetError) -> Error {
    match err {
        crate::waitset::WaitsetError::ResourceExhausted => Error::Capability(CapError::NoSpace),
        crate::waitset::WaitsetError::InvalidHandle => Error::Capability(CapError::InvalidSlot),
    }
}

/// Resolves a waitset cap slot to its kernel-local id, enforcing ownership (RFC-0033).
#[inline]
pub(super) fn waitset_id_from_cap(
    ctx: &mut Context<'_>,
    slot: usize,
) -> Result<crate::waitset::WaitsetId, Error> {
    let cap = ctx.tasks.current_caps_mut().get(slot)?;
    match cap.kind {
        CapabilityKind::Waitset(id) => {
            let ws_id = crate::waitset::WaitsetId(id);
            if !ctx.waitsets.owned_by(ws_id, ctx.tasks.current_pid().as_raw()) {
                return Err(Error::Capability(CapError::PermissionDenied));
            }
            Ok(ws_id)
        }
        _ => Err(Error::Capability(CapError::InvalidSlot)),
    }
}

/// `SYSCALL_WAITSET_CREATE` (38): allocate an empty waitset owned by the caller and
/// return its capability slot. Mirrors `sys_timer_create`'s alloc-then-cap pattern,
/// rolling back the table entry if the cap table is full.
pub(super) fn sys_waitset_create(ctx: &mut Context<'_>, _args: &Args) -> SysResult<usize> {
    let owner = ctx.tasks.current_pid().as_raw();
    let ws_id = ctx.waitsets.alloc(owner).map_err(map_waitset_error)?;
    let cap = Capability { kind: CapabilityKind::Waitset(ws_id.0), rights: Rights::MANAGE };
    // RFC-0069 slot discipline: a self-created object allocates from the TOP of
    // the table so it can never take a low slot that init's deterministic
    // post-spawn wiring is about to install into (the policyd waitset
    // collision → capability-denied → init abort).
    match ctx.tasks.current_caps_mut().allocate_high(cap) {
        Ok(slot) => Ok(slot),
        Err(err) => {
            let _ = ctx.waitsets.free(ws_id);
            Err(Error::Capability(err))
        }
    }
}

/// `SYSCALL_WAITSET_ADD` (39): add an endpoint as a waitset member. The caller must
/// hold `RECV` right on the endpoint (proves it is the legitimate receiver). Bounded to
/// `MAX_WAITSET_MEMBERS`; over-limit → `NoSpace`. Args: (waitset_slot, endpoint_slot).
pub(super) fn sys_waitset_add(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let ws_slot = args.get(0);
    let ep_slot = args.get(1);
    let ws_id = waitset_id_from_cap(ctx, ws_slot)?;
    let endpoint =
        ctx.tasks.current_caps_mut().derive_endpoint_ref(ep_slot, Rights::RECV)?.endpoint();
    ctx.waitsets.add_member(ws_id, endpoint).map_err(map_waitset_error)?;
    Ok(0)
}

/// `SYSCALL_WAITSET_WAIT` (40): block until any member endpoint has a pending message,
/// then return the ready member index (in add order). `deadline_ns == 0` blocks
/// indefinitely; a non-zero deadline yields `TimedOut`. Args: (waitset_slot, deadline_ns).
///
/// This is purely additive: it reuses the existing recv-waiter / wake / deadline
/// machinery. The task registers as a recv-waiter on *every* member; the first member a
/// sender delivers to wakes it (via the unchanged `router.send → pop_recv_waiter →
/// tasks.wake` path). The single-endpoint recv path is untouched.
pub(super) fn sys_waitset_wait(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let ws_slot = args.get(0);
    let deadline_ns = args.get(1) as u64;
    let ws_id = waitset_id_from_cap(ctx, ws_slot)?;

    if deadline_ns != 0 {
        ctx.timer.set_wakeup(deadline_ns);
    }

    // Snapshot the (bounded, ≤16) members into a stack buffer: no heap on the wait/block
    // path, and it detaches the member list from `ctx` so the router can be borrowed freely.
    let mut buf = [0u32; crate::waitset::MAX_WAITSET_MEMBERS];
    let n = match ctx.waitsets.members(ws_id) {
        Some(m) => {
            buf[..m.len()].copy_from_slice(m);
            m.len()
        }
        None => return Err(Error::Capability(CapError::InvalidSlot)),
    };
    let members = &buf[..n];
    let cur = ctx.tasks.current_pid();

    // Clear any registrations left from a prior blocking round: when a sender woke us via
    // one member, the *other* members still list us as a waiter. Idempotent on first entry.
    // Guarantees we hold no member registrations whenever we return ready/timed-out.
    for &ep in members {
        let _ = ctx.router.remove_recv_waiter(ep, cur.as_raw());
    }

    // Level-ready scan: first member with a pending message wins.
    if let Some(index) = crate::waitset::first_ready(members, |ep| ctx.router.pending(ep)) {
        return Ok(index);
    }
    if deadline_ns != 0 && ctx.timer.now() >= deadline_ns {
        return Err(Error::Ipc(ipc::IpcError::TimedOut));
    }

    // Register on every member, then re-scan once (a sender may have enqueued between the
    // scan above and registration — the missed-wakeup guard, as in sys_ipc_recv_v1).
    for &ep in members {
        let _ = ctx.router.register_recv_waiter(ep, cur.as_raw());
    }
    if let Some(index) = crate::waitset::first_ready(members, |ep| ctx.router.pending(ep)) {
        for &ep in members {
            let _ = ctx.router.remove_recv_waiter(ep, cur.as_raw());
        }
        return Ok(index);
    }

    ctx.tasks.block_current(BlockReason::Waitset { ws_id: ws_id.0, deadline_ns }, ctx.scheduler);
    wake_expired_blocked(ctx);
    if let Some(next) = ctx.scheduler.schedule_next() {
        ctx.tasks.set_current(next);
        return Err(Error::Reschedule);
    }
    // Degenerate fallback: nothing else runnable. Deregister, self-wake, reschedule — the
    // idle loop re-drives us (mirrors the single-endpoint recv path; load-bearing self-wake).
    for &ep in members {
        let _ = ctx.router.remove_recv_waiter(ep, cur.as_raw());
    }
    observe_wake_outcome(ctx.tasks.wake(cur, ctx.scheduler));
    Err(Error::Reschedule)
}

#[inline]
pub(super) fn map_fence_error(err: crate::fence::FenceError) -> Error {
    match err {
        crate::fence::FenceError::ResourceExhausted => Error::Capability(CapError::NoSpace),
        crate::fence::FenceError::InvalidHandle => Error::Capability(CapError::InvalidSlot),
    }
}

/// Resolves a fence cap slot to its kernel-local id. Authority IS cap
/// possession (RFC-0033): the slot lookup in the caller's own cap table
/// proves the fence was created by or transferred to this task — a
/// creator-pid check on top would render transferred fence caps unusable
/// (the workpool hands its job/done fences to same-AS worker threads).
/// The table's `owner_pid` stays lifecycle-only (free/teardown).
#[inline]
pub(super) fn fence_id_from_cap(
    ctx: &mut Context<'_>,
    slot: usize,
) -> Result<crate::fence::FenceId, Error> {
    let cap = ctx.tasks.current_caps_mut().get(slot)?;
    match cap.kind {
        CapabilityKind::Fence(id) => {
            let fence_id = crate::fence::FenceId(id);
            if !ctx.fences.exists(fence_id) {
                return Err(Error::Capability(CapError::InvalidSlot));
            }
            Ok(fence_id)
        }
        _ => Err(Error::Capability(CapError::InvalidSlot)),
    }
}

/// `SYSCALL_FENCE_CREATE` (41): allocate a fence (value 0) owned by the caller and return
/// its capability slot. Mirrors `sys_timer_create`'s alloc-then-cap rollback pattern.
pub(super) fn sys_fence_create(ctx: &mut Context<'_>, _args: &Args) -> SysResult<usize> {
    let owner = ctx.tasks.current_pid().as_raw();
    let fence_id = ctx.fences.alloc(owner).map_err(map_fence_error)?;
    let cap = Capability { kind: CapabilityKind::Fence(fence_id.0), rights: Rights::MANAGE };
    match ctx.tasks.current_caps_mut().allocate(cap) {
        Ok(slot) => Ok(slot),
        Err(err) => {
            let _ = ctx.fences.free(fence_id);
            Err(Error::Capability(err))
        }
    }
}

/// `SYSCALL_FENCE_SIGNAL` (42): advance the fence monotonically to at least `value` and wake
/// every waiter the new value now satisfies. Args: (fence_slot, value).
pub(super) fn sys_fence_signal(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let slot = args.get(0);
    let value = args.get(1) as u64;
    let fence_id = fence_id_from_cap(ctx, slot)?;
    ctx.fences.signal(fence_id, value).map_err(map_fence_error)?;
    // Wake satisfied waiters. Bounded stack buffer (no heap); any overflow is released by
    // the next signal. The woken tasks re-check `value >= target` on re-entry.
    let mut woken = [0u32; crate::fence::MAX_FENCE_WAITERS];
    let n = ctx.fences.take_satisfied(fence_id, &mut woken);
    for &pid in &woken[..n] {
        observe_wake_outcome(ctx.tasks.wake(task::Pid::from_raw(pid), ctx.scheduler));
    }
    Ok(0)
}

/// `SYSCALL_FENCE_WAIT` (43): block until the fence value reaches `target`. `deadline_ns == 0`
/// blocks indefinitely; a non-zero deadline yields `TimedOut`. Args: (fence_slot, target,
/// deadline_ns). Additive: reuses `BlockReason::Fence` + `tasks.wake` (driven by
/// `fence_signal`); the single-endpoint recv path is untouched.
pub(super) fn sys_fence_wait(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let slot = args.get(0);
    let target = args.get(1) as u64;
    let deadline_ns = args.get(2) as u64;
    let fence_id = fence_id_from_cap(ctx, slot)?;

    if deadline_ns != 0 {
        ctx.timer.set_wakeup(deadline_ns);
    }
    let cur = ctx.tasks.current_pid();

    // Clear any registration left from a prior blocking round (idempotent), so we hold no
    // waiter entry whenever we return satisfied/timed-out.
    ctx.fences.remove_waiter(fence_id, cur.as_raw());

    if ctx.fences.is_satisfied(fence_id, target) == Some(true) {
        return Ok(0);
    }
    if deadline_ns != 0 && ctx.timer.now() >= deadline_ns {
        return Err(Error::Ipc(ipc::IpcError::TimedOut));
    }

    // Register + recheck once (a signal may land between the check above and registration).
    ctx.fences.register_waiter(fence_id, cur.as_raw(), target).map_err(map_fence_error)?;
    if ctx.fences.is_satisfied(fence_id, target) == Some(true) {
        ctx.fences.remove_waiter(fence_id, cur.as_raw());
        return Ok(0);
    }

    ctx.tasks.block_current(
        BlockReason::Fence { fence_id: fence_id.0, target, deadline_ns },
        ctx.scheduler,
    );
    wake_expired_blocked(ctx);
    if let Some(next) = ctx.scheduler.schedule_next() {
        ctx.tasks.set_current(next);
        return Err(Error::Reschedule);
    }
    // Degenerate fallback: nothing else runnable — deregister, self-wake, reschedule.
    ctx.fences.remove_waiter(fence_id, cur.as_raw());
    observe_wake_outcome(ctx.tasks.wake(cur, ctx.scheduler));
    Err(Error::Reschedule)
}
