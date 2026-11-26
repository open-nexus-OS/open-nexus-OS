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

use super::ipc_ctx::{locked, Ipc, IpcResult, Outcome, Slept, Waits};
use super::*;

#[inline]
pub(super) fn map_timer_error(err: crate::timer::TimerError) -> Error {
    match err {
        crate::timer::TimerError::ResourceExhausted => Error::Capability(CapError::NoSpace),
        crate::timer::TimerError::InvalidHandle => Error::Capability(CapError::InvalidSlot),
        crate::timer::TimerError::AlreadyArmed => Error::Capability(CapError::PermissionDenied),
    }
}

/// The timer the caller's capability in `slot` names: `MANAGE`, and a timer the caller owns.
/// The table is held for the one read; the timer's own check comes after it.
fn timer_id_from_cap(ipc: &Ipc<'_>, slot: usize) -> Result<crate::timer::TimerId, Error> {
    let cap = ipc.caps().get(slot)?;
    if !cap.rights.contains(Rights::MANAGE) {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    match cap.kind {
        CapabilityKind::Timer(id) => {
            let timer_id = crate::timer::TimerId(id);
            if !ipc.timers.owned_by(timer_id, ipc.me.as_raw()) {
                return Err(Error::Capability(CapError::PermissionDenied));
            }
            Ok(timer_id)
        }
        _ => Err(Error::Capability(CapError::InvalidSlot)),
    }
}

/// The endpoint `slot` names with any rights, pinned for a store of its id
/// (`ipc_reclaim::stored`); a slot that names something else is `InvalidSlot`.
fn pin_bound_endpoint(
    ctx: &mut Context<'_>,
    slot: usize,
) -> Result<crate::cap::PinnedEndpoint, Error> {
    ctx.tasks.current_caps_mut().pin_endpoint(slot, Rights::empty()).map_err(|err| match err {
        // With no right asked for, a refusal is the kind's.
        CapError::PermissionDenied => Error::Capability(CapError::InvalidSlot),
        err => Error::Capability(err),
    })
}

pub(super) fn sys_timer_create(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let notify_slot = args.get(0);
    let interval_ns = args.get(1) as u64;
    let notify = pin_bound_endpoint(ctx, notify_slot)?;
    let timer_id = ctx
        .hart_timers
        .alloc(ctx.tasks.current_pid().as_raw(), notify.id(), interval_ns)
        .map_err(map_timer_error)?;
    // A timer of a dead endpoint would fire into the slot's next tenant: its death freed it.
    if !super::ipc_reclaim::stored(ctx.router, ctx.waitsets, ctx.hart_timers, &notify) {
        return Err(Error::Ipc(ipc::IpcError::NoSuchEndpoint));
    }
    let timer_cap = Capability { kind: CapabilityKind::Timer(timer_id.0), rights: Rights::MANAGE };
    // The table is held for the one change: the refusal's cleanup runs without its lock.
    let installed =
        Held::plain(timer_cap).and_then(|held| ctx.tasks.current_caps_mut().allocate(held));
    match installed {
        Ok(slot) => Ok(slot),
        Err(err) => {
            let _ = ctx.hart_timers.free(timer_id);
            Err(Error::Capability(err))
        }
    }
}

pub(super) fn sys_timer_set(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let out = timer_set(&ctx.ipc()?, args);
    locked(ctx, out)
}

/// The timer set's body (TASK-0330 P4: the caller's table, then the timer table — each its
/// own lock). Args: (timer_slot, deadline_ns).
pub(super) fn timer_set(ipc: &Ipc<'_>, args: &Args) -> IpcResult {
    let deadline_ns = args.get(1) as u64;
    let timer_id = timer_id_from_cap(ipc, args.get(0))?;
    // TASK-0363 P8: the timer is this hart's — it arms the wakeup, its own floor carries it.
    let cpu = crate::smp::cpu_current_id().as_index() as u32;
    ipc.timers.arm(timer_id, deadline_ns, cpu).map_err(map_timer_error)?;
    crate::trap::arm_wakeup(ipc.timer, deadline_ns);
    Ok(Outcome::Done(0))
}

pub(super) fn sys_timer_cancel(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let out = timer_cancel(&ctx.ipc()?, args);
    locked(ctx, out)
}

/// The timer cancel's body. Args: (timer_slot).
pub(super) fn timer_cancel(ipc: &Ipc<'_>, args: &Args) -> IpcResult {
    let timer_id = timer_id_from_cap(ipc, args.get(0))?;
    ipc.timers.disarm(timer_id).map_err(map_timer_error)?;
    Ok(Outcome::Done(0))
}

/// Binds an external interrupt (PLIC source) to an endpoint the caller owns, so
/// the kernel routes that device IRQ to the driver's endpoint and wakes it. The
/// caller must hold an Endpoint capability for the receiving endpoint (proves it
/// is the intended driver). The line is enabled where the caller is homed and follows it
/// (RFC-0118); on a machine where no online CPU has a controller context it cannot fire and
/// the bind is refused (`EINVAL`). Args: (irq_source_id, endpoint_cap_slot).
pub(super) fn sys_irq_bind(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let irq_raw = args.get(0) as u32;
    let endpoint_slot = args.get(1);
    let irq =
        crate::hal::plic::IrqId::new(irq_raw).ok_or(Error::Capability(CapError::InvalidSlot))?;
    // RFC-0098 C3: a task binds only the line its device capability carries.
    if !ctx.tasks.current_caps_mut().holds_device_irq(irq_raw) {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    // TASK-0330 S2a: the console's own line is the kernel's.
    if crate::hal::platform::console_irq() == Some(irq) {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    let endpoint = pin_bound_endpoint(ctx, endpoint_slot)?;
    let owner = ctx.tasks.current_pid();
    let home = ctx.tasks.current_task().home_cpu();
    let refused = || Error::AddressSpace(AddressSpaceError::InvalidArgs);
    crate::irq::bind(irq, endpoint.id(), owner, home).ok_or_else(refused)?;
    // A dead endpoint's line is released by its death, as if the bind had been refused.
    if !super::ipc_reclaim::stored(ctx.router, ctx.waitsets, ctx.hart_timers, &endpoint) {
        return Err(refused());
    }
    Ok(0)
}

/// `SYSCALL_CHILD_EXIT_BIND` (62, TASK-0333 6a): binds an endpoint the caller holds a SEND
/// capability to as the sink for its children's deaths — the exit funnel writes one
/// `OP_CHILD_EXITED` frame per dead child into it, the shape of a timer's fire and an
/// interrupt's. The reaper of record (execd, RFC-0081) waits on it instead of sweeping on its
/// next request: a process's death is an event, never something polled. Args:
/// (endpoint_slot); slot `usize::MAX` clears the binding. Only the caller's own binding.
pub(super) fn sys_child_exit_bind(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let endpoint_slot = args.get(0);
    // The binding holds the endpoint the capability named (`cap::PinnedEndpoint`), judged and
    // held in one look at the slot: no death of it has to find the binding.
    let endpoint = if endpoint_slot == usize::MAX {
        None
    } else {
        Some(ctx.tasks.current_caps_mut().pin_endpoint(endpoint_slot, Rights::SEND)?)
    };
    ctx.tasks.current_task_mut().set_exit_notify(endpoint);
    Ok(0)
}

/// `SYSCALL_MEM_PRESSURE_BIND` (67, ADR-0085, TASK-0333 6e-0): binds an endpoint the caller
/// holds a SEND capability to as the kernel's one memory pressure sink — a level change writes
/// one `OP_MEMORY_PRESSURE` frame into it, the shape of a child's death and an interrupt's. The
/// first binder holds the sink until its endpoint dies; another process is refused. Args:
/// (endpoint_slot); slot `usize::MAX` clears the caller's own binding.
pub(super) fn sys_mem_pressure_bind(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let endpoint_slot = args.get(0);
    let pid = ctx.tasks.current_pid();
    let denied = || Error::Capability(CapError::PermissionDenied);
    if endpoint_slot == usize::MAX {
        return crate::mm::pressure_endpoint::unbind(pid).map(|()| 0).map_err(|_| denied());
    }
    let endpoint = ctx.tasks.current_caps_mut().pin_endpoint(endpoint_slot, Rights::SEND)?;
    crate::mm::pressure_endpoint::bind(endpoint.id(), pid).map_err(|_| denied())?;
    // A dead endpoint would hold the sink with nobody to free it: its death cleared it.
    if !super::ipc_reclaim::stored(ctx.router, ctx.waitsets, ctx.hart_timers, &endpoint) {
        return Err(denied());
    }
    Ok(0)
}

/// Acknowledges a delivered IRQ so the PLIC re-arms it. The driver calls this
/// after it has drained the device (cleared the interrupt condition). Args:
/// (irq_source_id).
pub(super) fn sys_irq_complete(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let irq_raw = args.get(0) as u32;
    let irq =
        crate::hal::plic::IrqId::new(irq_raw).ok_or(Error::Capability(CapError::InvalidSlot))?;
    // Re-arming a line is the same authority as binding it.
    if !ctx.tasks.current_caps_mut().holds_device_irq(irq_raw) {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
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
    ipc: &Ipc<'_>,
    slot: usize,
) -> Result<crate::waitset::WaitsetId, Error> {
    let cap = ipc.caps().get(slot)?;
    match cap.kind {
        CapabilityKind::Waitset(id) => {
            let ws_id = crate::waitset::WaitsetId(id);
            if !ipc.waitsets.owned_by(ws_id, ipc.me.as_raw()) {
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
    let installed =
        Held::plain(cap).and_then(|held| ctx.tasks.current_caps_mut().allocate_high(held));
    match installed {
        Ok(slot) => Ok(slot),
        Err(err) => {
            let _ = ctx.waitsets.free(ws_id);
            Err(Error::Capability(err))
        }
    }
}

/// `SYSCALL_WAITSET_ADD` (39): add an endpoint as a waitset member. A capability with the
/// `RECV` right adds a RECEIVER's member (ready when a message is pending — the right proves the
/// caller is the legitimate receiver). A SEND-only capability arms a SENDER's member (TASK-0339,
/// RFC-0033 amendment): the caller found the queue full; the member is ready once the queue has
/// room, or no receiver remains, and is reported once — adding it again re-arms it. Bounded to
/// `MAX_WAITSET_MEMBERS`; over-limit → `NoSpace`. Args: (waitset_slot, endpoint_slot).
pub(super) fn sys_waitset_add(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let ws_slot = args.get(0);
    let ep_slot = args.get(1);
    let ws_id = waitset_id_from_cap(&ctx.ipc()?, ws_slot)?;
    // Each look at the table is its own hold: the waitset is changed without the table's lock.
    // The endpoint is pinned for the store; a member of one that died meanwhile is dropped by
    // its death (it would never be ready — and must not name the slot's next tenant).
    let receiver = ctx.tasks.current_caps_mut().pin_endpoint(ep_slot, Rights::RECV);
    let endpoint = match receiver {
        Ok(endpoint) => {
            ctx.waitsets.add_member(ws_id, endpoint.id()).map_err(map_waitset_error)?;
            endpoint
        }
        Err(_) => {
            let endpoint = ctx.tasks.current_caps_mut().pin_endpoint(ep_slot, Rights::SEND)?;
            ctx.waitsets.arm_writable(ws_id, endpoint.id()).map_err(map_waitset_error)?;
            endpoint
        }
    };
    let _ = super::ipc_reclaim::stored(ctx.router, ctx.waitsets, ctx.hart_timers, &endpoint);
    Ok(0)
}

/// Whether member word `m` is ready: a receiver's when a message (or the EOF latch) is pending,
/// an armed sender's when its queue has room (TASK-0339).
fn member_ready(router: &ipc::Router, m: u32) -> bool {
    let ep = crate::waitset::member_endpoint(m);
    if crate::waitset::member_writable(m) {
        crate::waitset::member_armed(m) && router.writable(ep)
    } else {
        router.pending(ep) || router.eof_pending(ep)
    }
}

/// Registers `pid` on every member: a receiver's as a recv-waiter (a send wakes it), an armed
/// sender's as a writable waiter (every receive that frees room wakes it).
fn register_members(router: &ipc::Router, members: &[u32], pid: u32) {
    for &m in members {
        let ep = crate::waitset::member_endpoint(m);
        if !crate::waitset::member_writable(m) {
            let _ = router.register_recv_waiter(ep, pid);
        } else if crate::waitset::member_armed(m) {
            let _ = router.register_writable_waiter(ep, pid);
        }
    }
}

/// Clears `pid`'s registrations on every member (idempotent) — before a scan, after a ready
/// report, on a deadline (`api::wake_expired_blocked`).
pub(super) fn deregister_members(router: &ipc::Router, members: &[u32], pid: u32) {
    for &m in members {
        let ep = crate::waitset::member_endpoint(m);
        if crate::waitset::member_writable(m) {
            router.remove_writable_waiter(ep, pid);
        } else {
            let _ = router.remove_recv_waiter(ep, pid);
        }
    }
}

/// A receive took a message from `endpoint` and freed room: the first parked sender wakes to
/// retry its send (FIFO, one per receive), and every waitset waiting for room wakes to report
/// its armed member (TASK-0339) — from a list apart, so it never takes the parked sender's wake.
pub(crate) fn wake_after_recv(router: &ipc::Router, endpoint: ipc::EndpointId) {
    if let Ok(Some(waiter)) = router.pop_send_waiter(endpoint) {
        super::ipc_ctx::wake(waiter);
    }
    while let Some(waiter) = router.pop_writable_waiter(endpoint) {
        super::ipc_ctx::wake(waiter);
    }
}

/// The wait reports member `index`: a sender's arm is spent there (told once).
fn report(ipc: &Ipc<'_>, ws_id: crate::waitset::WaitsetId, m: &[u32], index: usize) -> IpcResult {
    if m.get(index).is_some_and(|&w| crate::waitset::member_writable(w)) {
        ipc.waitsets.disarm(ws_id, index);
    }
    Ok(Outcome::Done(index))
}

/// The table's entry of the waitset wait (no user memory: the body as it is).
pub(super) fn sys_waitset_wait(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let out = waitset_wait(&ctx.ipc()?, args);
    locked(ctx, out)
}

/// `SYSCALL_WAITSET_WAIT` (40): block until any member is ready — a receiver's member has a
/// pending message, an armed sender's member has room — then return the ready member index (in
/// add order). `deadline_ns == 0` blocks indefinitely; a non-zero deadline yields `TimedOut`.
/// Args: (waitset_slot, deadline_ns).
///
/// It reuses the existing waiter / wake / deadline machinery. The task registers as a
/// recv-waiter on every receiver's member and as a writable waiter on every armed sender's
/// member; the first send to the former (`router.send → pop_recv_waiter`) or receive from the
/// latter (`wake_after_recv`, which wakes the writable waiters beside the first parked sender)
/// wakes it. A blocking send or receive is otherwise untouched.
pub(super) fn waitset_wait(ipc: &Ipc<'_>, args: &Args) -> IpcResult {
    let ws_slot = args.get(0);
    let deadline_ns = args.get(1) as u64;
    let ws_id = waitset_id_from_cap(ipc, ws_slot)?;

    if deadline_ns != 0 {
        crate::trap::arm_wakeup(ipc.timer, deadline_ns);
    }

    // Snapshot the (bounded, ≤16) members into a stack buffer: no heap on the wait/block
    // path, and the list is read under the waitset's lock once.
    let mut buf = [0u32; crate::waitset::MAX_WAITSET_MEMBERS];
    let Some(n) = ipc.waitsets.members(ws_id, &mut buf) else {
        return Err(Error::Capability(CapError::InvalidSlot));
    };
    let members = &buf[..n];
    let me = ipc.me.as_raw();
    let ready = || crate::waitset::first_ready(members, |m| member_ready(ipc.router, m));

    loop {
        // Clear any registrations left from a prior blocking round: when a peer woke us via
        // one member, the *other* members still list us as a waiter. Idempotent on first
        // entry. Guarantees we hold no member registrations whenever we return ready or
        // timed-out.
        deregister_members(ipc.router, members, me);

        // Level-ready scan: the first ready member wins.
        if let Some(index) = ready() {
            return report(ipc, ws_id, members, index);
        }
        if deadline_ns != 0 && ipc.timer.now() >= deadline_ns {
            return Err(Error::Ipc(ipc::IpcError::TimedOut));
        }

        // Prepare, register on every member, then scan once more: a peer may have sent or
        // received between the scan above and the registration — and one that does so from
        // here on finds the thread preparing or blocked (ADR-0102).
        ipc.prepare(BlockReason::Waitset { ws_id: ws_id.0, deadline_ns });
        register_members(ipc.router, members, me);
        if let Some(index) = ready() {
            ipc.abort();
            deregister_members(ipc.router, members, me);
            return report(ipc, ws_id, members, index);
        }
        match ipc.sleep() {
            Slept::Blocked => return Ok(Outcome::Blocked(Waits::Waitset(ws_id))),
            Slept::Order => {
                deregister_members(ipc.router, members, me);
                return Ok(Outcome::Locked);
            }
            Slept::Woken => {}
        }
    }
}

#[inline]
pub(super) fn map_fence_error(err: crate::fence::FenceError) -> Error {
    match err {
        crate::fence::FenceError::ResourceExhausted => Error::Capability(CapError::NoSpace),
        crate::fence::FenceError::InvalidHandle => Error::Capability(CapError::InvalidSlot),
    }
}

/// Resolves a fence cap slot to its kernel-local id and enforces the right the operation needs
/// (ADR-0062). Authority IS cap possession plus its RIGHTS (RFC-0033): the slot lookup in the
/// caller's own cap table proves the fence was created by or transferred to this task — a
/// creator-pid check on top would render transferred fence caps unusable (the workpool hands its
/// job/done fences to same-AS worker threads). `signal` demands `MANAGE`, `wait` accepts `WAIT`
/// too, so a child holding a WAIT-only stage fence can block for a stage but never release one.
/// The table's `owner_pid` stays lifecycle-only (free/teardown).
#[inline]
pub(super) fn fence_id_from_cap(
    ipc: &Ipc<'_>,
    slot: usize,
    any_of: Rights,
) -> Result<crate::fence::FenceId, Error> {
    let cap = ipc.caps().get(slot)?;
    if !cap.rights.intersects(any_of) {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    match cap.kind {
        CapabilityKind::Fence(id) => {
            let fence_id = crate::fence::FenceId(id);
            if !ipc.fences.exists(fence_id) {
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
    // The creator may signal AND wait; a derived WAIT-only copy is what init hands its children
    // (a subset of these rights — `CapTable::derive` refuses anything wider).
    let cap = Capability {
        kind: CapabilityKind::Fence(fence_id.0),
        rights: Rights::MANAGE | Rights::WAIT,
    };
    let installed = Held::plain(cap).and_then(|held| ctx.tasks.current_caps_mut().allocate(held));
    match installed {
        Ok(slot) => Ok(slot),
        Err(err) => {
            let _ = ctx.fences.free(fence_id);
            Err(Error::Capability(err))
        }
    }
}

/// `SYSCALL_FENCE_SIGNAL` (42): advance the fence monotonically to at least `value` and wake
/// every waiter the new value now satisfies. Requires `MANAGE` (ADR-0062: a WAIT-only holder —
/// every child of init holding the boot-stage fence — must not release a stage). Args:
/// (fence_slot, value).
pub(super) fn sys_fence_signal(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let out = fence_signal(&ctx.ipc()?, args);
    locked(ctx, out)
}

/// The fence signal's body (TASK-0330 P2: the fence's lock, the waiters' records).
pub(super) fn fence_signal(ipc: &Ipc<'_>, args: &Args) -> IpcResult {
    let slot = args.get(0);
    let value = args.get(1) as u64;
    let fence_id = fence_id_from_cap(ipc, slot, Rights::MANAGE)?;
    ipc.fences.signal(fence_id, value).map_err(map_fence_error)?;
    // Wake satisfied waiters. Bounded stack buffer (no heap); any overflow is released by
    // the next signal. The woken tasks re-check `value >= target` on re-entry.
    let mut woken = [0u32; crate::fence::MAX_FENCE_WAITERS];
    let n = ipc.fences.take_satisfied(fence_id, &mut woken);
    for &pid in &woken[..n] {
        ipc.wake(pid);
    }
    Ok(Outcome::Done(0))
}

/// `SYSCALL_FENCE_WAIT` (43): block until the fence value reaches `target`. `deadline_ns == 0`
/// blocks indefinitely; a non-zero deadline yields `TimedOut`. Args: (fence_slot, target,
/// deadline_ns). Additive: reuses `BlockReason::Fence` + `tasks.wake` (driven by
/// `fence_signal`); the single-endpoint recv path is untouched.
pub(super) fn sys_fence_wait(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let out = fence_wait(&ctx.ipc()?, args);
    locked(ctx, out)
}

/// The fence wait's body (TASK-0330 P2: prepare → register → look again → sleep).
pub(super) fn fence_wait(ipc: &Ipc<'_>, args: &Args) -> IpcResult {
    let slot = args.get(0);
    let target = args.get(1) as u64;
    let deadline_ns = args.get(2) as u64;
    let fence_id = fence_id_from_cap(ipc, slot, Rights::WAIT | Rights::MANAGE)?;

    if deadline_ns != 0 {
        crate::trap::arm_wakeup(ipc.timer, deadline_ns);
    }
    let me = ipc.me.as_raw();
    let satisfied = || ipc.fences.is_satisfied(fence_id, target) == Some(true);

    loop {
        // Clear any registration left from a prior blocking round (idempotent), so we hold
        // no waiter entry whenever we return satisfied/timed-out.
        ipc.fences.remove_waiter(fence_id, me);

        if satisfied() {
            return Ok(Outcome::Done(0));
        }
        if deadline_ns != 0 && ipc.timer.now() >= deadline_ns {
            return Err(Error::Ipc(ipc::IpcError::TimedOut));
        }

        // Prepare, register, look again (a signal may land between the check above and the
        // registration; one that lands from here on finds the thread preparing or blocked).
        ipc.prepare(BlockReason::Fence { fence_id: fence_id.0, target, deadline_ns });
        if let Err(err) = ipc.fences.register_waiter(fence_id, me, target) {
            ipc.abort();
            return Err(map_fence_error(err));
        }
        if satisfied() {
            ipc.abort();
            ipc.fences.remove_waiter(fence_id, me);
            return Ok(Outcome::Done(0));
        }
        match ipc.sleep() {
            Slept::Blocked => return Ok(Outcome::Blocked(Waits::Fence(fence_id))),
            Slept::Order => {
                ipc.fences.remove_waiter(fence_id, me);
                return Ok(Outcome::Locked);
            }
            Slept::Woken => {}
        }
    }
}
