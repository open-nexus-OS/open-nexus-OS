// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: what an IPC syscall works on and how it ends (TASK-0330 P2, ADR-0102). Send,
//! receive, call, reply, a waitset's wait and a fence's wait and signal touch the caller's
//! scheduling record, the router and the two tables — each behind its own lock — and user
//! memory through `mm::user_copy`. So one body serves both entries: the trap's entry before
//! the kernel lock (`trap::ipc_entry`), and the syscall table under it (a selftest, a
//! kernel-mode task, or the re-run of a body that asked for the lock).
//!
//! A body asks for the lock (`Outcome::Locked`) when a page of the caller's is not ready (the
//! locked entry commits it first, `ensure_user_range`) or an order waits for the caller (a
//! kill, a freeze — the locked entry runs it first). It has changed nothing by then, or has
//! put it back: the syscall re-executes whole.
//!
//! A block is `prepare` → register on the object → look again → `sleep`. After a `sleep`
//! that answered `Blocked` the hart touches the thread no more: its record is another hart's
//! to dispatch.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the wait word's rule and the table walk on the host (`sync::wait_word`,
//!   `user_walk`); every QEMU lane's IPC ladder, `KSELFTEST: user copy ok`
//! ADR: docs/adr/0102-a-thread-blocks-in-two-steps-and-a-waker-never-waits-ipc-leaves-the-kernel-lock.md

use super::*;
use crate::sched::record::Record;
use crate::sync::wait_word::Commit;

/// What an IPC syscall works on: nothing here is the kernel lock's.
pub struct Ipc<'a> {
    /// The caller.
    pub me: task::Pid,
    /// Its scheduling record: its capability table, its wait, its saved frame.
    pub rec: &'static Record,
    pub router: &'a ipc::Router,
    pub waitsets: &'a crate::waitset::WaitsetTable,
    pub fences: &'a crate::fence::FenceTable,
    pub timer: &'a dyn Timer,
    /// The timer capabilities, behind their own lock (TASK-0330 P4).
    pub timers: &'a crate::timer::HartTimers,
    /// The address spaces, each behind its own lock (TASK-0330 P3): what a mapping works on.
    pub spaces: &'a AddressSpaceManager,
    /// The syscall entered before the kernel lock (`trap::ipc_entry`): when the caller blocks,
    /// its hart leaves the caller's space itself — under the lock the trap's exit does.
    pub free: bool,
}

/// What a blocked caller is registered on — what a self-wake before the scheduler loop
/// exists takes back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Waits {
    Recv(ipc::EndpointId),
    Send(ipc::EndpointId),
    Waitset(crate::waitset::WaitsetId),
    Fence(crate::fence::FenceId),
}

/// How an IPC syscall's body ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Its return value.
    Done(usize),
    /// The caller is blocked and off this hart; the syscall re-executes when it runs again.
    Blocked(Waits),
    /// It returns 0 and gives the hart up to whatever waits on it (`yield`).
    Yielded,
    /// It needs the kernel lock (a page not ready, an order to run first); nothing is changed.
    Locked,
}

pub type IpcResult = Result<Outcome, Error>;

/// How the second step of a block ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slept {
    /// Blocked: the caller returns `Outcome::Blocked` and touches the thread no more.
    Blocked,
    /// A wake came between the steps: look again at what it waited for.
    Woken,
    /// An order waits for the thread (a kill, a freeze): take the registration back and ask
    /// for the lock.
    Order,
}

impl Ipc<'_> {
    /// The caller's capability table, for one look or one change.
    pub fn caps(&self) -> task::Caps {
        self.rec.caps()
    }

    /// The root of the caller's page table, if it has a space. The hart is about to read
    /// through that space's tables, so the space's shootdowns reach it from here on (TASK-0330
    /// P3) — it is in the set already when it runs the caller, which is the usual case.
    fn root(&self) -> Option<usize> {
        const PPN: usize = (1 << 44) - 1;
        let view = self.rec.space()?;
        if let Some(harts) = crate::hart_spaces::installed() {
            harts.reads_through(crate::smp::cpu_current_id().as_index(), view.raw);
        }
        Some((view.satp & PPN) << 12)
    }

    /// Copies `dst.len()` bytes from the caller's memory at `va`; `false`: a page of it is not
    /// ready — the body asks for the lock.
    #[must_use]
    pub fn read(&self, va: usize, dst: &mut [u8]) -> bool {
        self.root().is_some_and(|root| crate::mm::user_copy::copy_in(root, va, dst).is_ok())
    }

    /// Copies `src` into the caller's memory at `va`; `false` as for `read`.
    #[must_use]
    pub fn write(&self, va: usize, src: &[u8]) -> bool {
        self.root().is_some_and(|root| crate::mm::user_copy::copy_out(root, va, src).is_ok())
    }

    /// [`Self::read`] into `len` bytes at `to` — a memory object's frame through the direct map.
    ///
    /// # Safety
    /// As `user_copy::copy_in_raw`: the caller holds the frames `to..to + len` lies in.
    #[must_use]
    pub unsafe fn read_raw(&self, va: usize, to: *mut u8, len: usize) -> bool {
        // SAFETY: the caller's contract, handed on.
        self.root().is_some_and(|root| unsafe {
            crate::mm::user_copy::copy_in_raw(root, va, to, len).is_ok()
        })
    }

    /// [`Self::write`] from `len` bytes at `from` — a memory object's frame.
    ///
    /// # Safety
    /// As `user_copy::copy_out_raw`: the caller holds the frames `from..from + len` lies in.
    #[must_use]
    pub unsafe fn write_raw(&self, va: usize, from: *const u8, len: usize) -> bool {
        // SAFETY: the caller's contract, handed on.
        self.root().is_some_and(|root| unsafe {
            crate::mm::user_copy::copy_out_raw(root, va, from, len).is_ok()
        })
    }

    /// Makes the caller's pages of `va..va + len` ready for `access`, each with the caller's
    /// own space held and nothing else — what its fault would do (`populate::fill_free`).
    /// `false`: the kernel lock's path fills from the start, where a refusal is named, a
    /// squeezed page inflated and a swapped one waited for. The caller holds no ranked lock.
    #[must_use]
    pub fn fill(&self, va: usize, len: usize, access: crate::lazy::Access) -> bool {
        let (Ok(handle), Some(end)) = (self.space(), va.checked_add(len)) else { return false };
        let mut page = va & !(crate::mm::PAGE_SIZE - 1);
        while page < end {
            if !crate::mm::populate::fill_free(self.spaces, handle, page, access) {
                return false;
            }
            page += crate::mm::PAGE_SIZE;
        }
        true
    }

    /// The caller's address space.
    pub fn space(&self) -> Result<crate::mm::AsHandle, Error> {
        let view = self.rec.space().ok_or(AddressSpaceError::InvalidHandle)?;
        crate::mm::AsHandle::from_raw(view.raw).ok_or(AddressSpaceError::InvalidHandle.into())
    }

    /// Wakes `pid` (a waiter taken off an object).
    pub fn wake(&self, pid: u32) {
        wake(pid);
    }

    /// The first step of a block.
    pub fn prepare(&self, reason: BlockReason) {
        task::wake::prepare_block(self.me, reason);
    }

    /// The caller found its answer after `prepare`: it does not wait.
    pub fn abort(&self) {
        task::wake::abort_block(self.me);
    }

    /// The second step. An order left for the thread is read here, after `prepare`: the hart
    /// that leaves one wakes the thread after it wrote the word (`TaskTable::mark_pending`), so
    /// it is either seen here or its wake ends the wait.
    #[must_use]
    pub fn sleep(&self) -> Slept {
        if self.rec.pending().is_some() {
            self.abort();
            return Slept::Order;
        }
        // Off the lock the hart is on the kernel's space before anyone finds the thread blocked:
        // its kill and reap on another hart then free a space no hart names.
        let leave = || {
            if self.free {
                crate::hart_spaces::enter_kernel_space();
            }
        };
        match task::wake::commit_block(self.me, &mut Scheduler::unlocked(), leave) {
            Commit::Blocked => Slept::Blocked,
            Commit::Woken => Slept::Woken,
        }
    }
}

/// Wakes `pid`: its record and the run queues (`task::wake::wake`).
pub(super) fn wake(pid: u32) {
    observe_wake_outcome(task::wake::wake(task::Pid::from_raw(pid), &mut Scheduler::unlocked()));
}

impl Waits {
    /// Takes `pid`'s registration back.
    fn withdraw(self, ipc: &Ipc<'_>) {
        let pid = ipc.me.as_raw();
        match self {
            Self::Recv(endpoint) => {
                let _ = ipc.router.remove_recv_waiter(endpoint, pid);
            }
            Self::Send(endpoint) => {
                let _ = ipc.router.remove_send_waiter(endpoint, pid);
            }
            Self::Waitset(id) => {
                let mut buf = [0u32; crate::waitset::MAX_WAITSET_MEMBERS];
                let n = ipc.waitsets.members(id, &mut buf).unwrap_or(0);
                super::sync_objects::deregister_members(ipc.router, &buf[..n], pid);
            }
            Self::Fence(id) => ipc.fences.remove_waiter(id, pid),
        }
    }
}

/// What a receive found on its endpoint.
pub(super) enum Taken {
    Message(ipc::Message),
    /// No message: the body ends with this outcome.
    Ended(Outcome),
}

/// Takes the next message from `endpoint`, or blocks the caller on it (prepare → register →
/// look again → sleep). `eof`: the v1 receive's last-sender verdict (RFC-0079) with its opt-in
/// flag; `None` for a receive that has none.
pub(super) fn take(
    ipc: &Ipc<'_>,
    endpoint: ipc::EndpointId,
    nonblock: bool,
    deadline_ns: u64,
    eof: Option<bool>,
) -> Result<Taken, Error> {
    use crate::ipc_eof::EmptyQueue;
    let me = ipc.me.as_raw();
    let verdict = |nonblock: bool| {
        eof.map(|opt| super::eof_scan::empty_queue_verdict(ipc.router, endpoint, nonblock, opt))
    };
    loop {
        match ipc.router.recv(endpoint) {
            Ok(msg) => {
                // Receiving frees queue capacity: wake one parked sender and the waitsets
                // waiting for room (TASK-0339).
                wake_after_recv(ipc.router, endpoint);
                return Ok(Taken::Message(msg));
            }
            Err(ipc::IpcError::QueueEmpty) if !nonblock => {
                ipc.router.clear_eof_pending(endpoint);
                if deadline_ns != 0 && ipc.timer.now() >= deadline_ns {
                    return Err(Error::Ipc(ipc::IpcError::TimedOut));
                }
                ipc.prepare(BlockReason::IpcRecv { endpoint, deadline_ns });
                // IMPORTANT: if the endpoint is gone, do not block (would deadlock forever).
                if let Err(e) = ipc.router.register_recv_waiter(endpoint, me) {
                    ipc.abort();
                    return Err(e.into());
                }
                // Avoid missed-wakeup: a sender can enqueue between our empty check and waiter
                // registration. Look again once registered; a message there is taken without
                // blocking.
                let found = match ipc.router.recv(endpoint) {
                    Ok(msg) => Some(Ok(msg)),
                    Err(ipc::IpcError::QueueEmpty) => {
                        // RFC-0079 last-sender EOF (opt-in): the one host-tested verdict.
                        (verdict(false) == Some(EmptyQueue::PeerClosed))
                            .then_some(Err(ipc::IpcError::PeerClosed))
                    }
                    Err(e) => Some(Err(e)),
                };
                if let Some(found) = found {
                    ipc.abort();
                    let _ = ipc.router.remove_recv_waiter(endpoint, me);
                    // A sender queues its last message before it lets go of the endpoint: one
                    // that arrived between the look and the verdict is taken first.
                    if found.is_err() && ipc.router.pending(endpoint) {
                        continue;
                    }
                    let msg = found?;
                    wake_after_recv(ipc.router, endpoint);
                    return Ok(Taken::Message(msg));
                }
                match ipc.sleep() {
                    Slept::Blocked => {
                        return Ok(Taken::Ended(Outcome::Blocked(Waits::Recv(endpoint))))
                    }
                    Slept::Order => {
                        let _ = ipc.router.remove_recv_waiter(endpoint, me);
                        return Ok(Taken::Ended(Outcome::Locked));
                    }
                    Slept::Woken => {
                        let _ = ipc.router.remove_recv_waiter(endpoint, me);
                    }
                }
            }
            Err(e) => {
                if matches!(e, ipc::IpcError::QueueEmpty) {
                    // TASK-0333 6a: the verdict does not depend on the wait mode — a waitset
                    // receiver (non-blocking after its wake) learns of its last peer's death
                    // here, instead of consuming the latch and parking forever.
                    let verdict = verdict(true);
                    ipc.router.clear_eof_pending(endpoint);
                    if verdict == Some(EmptyQueue::PeerClosed) {
                        if ipc.router.pending(endpoint) {
                            continue;
                        }
                        return Err(Error::Ipc(ipc::IpcError::PeerClosed));
                    }
                }
                return Err(e.into());
            }
        }
    }
}

/// Where a receive writes in the caller's memory.
pub(super) struct Out {
    pub header_ptr: usize,
    pub payload_ptr: usize,
    pub payload_max: usize,
    /// Where the sender's kernel-derived service identity goes (the descriptor receive).
    pub sender_ptr: Option<usize>,
    pub truncate: bool,
}

/// Hands a taken message to the caller: a moved capability into its table (its slot written
/// into the header's `src` — the capability arrives as the sender held it, RFC-0102), then
/// header, sender and payload into its memory. A page of the caller's not ready undoes it:
/// the capability leaves the table again, the message returns to the head of its queue, and
/// the body asks for the lock.
pub(super) fn deliver(
    ipc: &Ipc<'_>,
    endpoint: ipc::EndpointId,
    mut msg: ipc::Message,
    out: &Out,
) -> IpcResult {
    let src = msg.header.src;
    let mut placed = None;
    if let Some(held) = msg.moved_cap.take() {
        #[cfg(feature = "ipc_trace_ring")]
        let moved_ep = match held.cap().kind {
            CapabilityKind::Endpoint(id) => id,
            _ => 0,
        };
        // The table's lock is released before the refusal's requeue takes the endpoint's.
        let slot = ipc.caps().try_allocate(held);
        match slot {
            Ok(slot) => {
                #[cfg(feature = "ipc_trace_ring")]
                crate::ipc::trace::record_capmove_alloc(
                    ipc.me.as_raw(),
                    endpoint,
                    slot as u32,
                    moved_ep,
                );
                msg.header.src = slot as u32;
                placed = Some(slot);
            }
            Err((_, held)) => {
                // The receiver cannot accept a moved cap right now (no free cap slot): the
                // message is queued again and the caller gets a stable error (ENOSPC).
                msg.moved_cap = Some(held);
                let _ = ipc.router.requeue_front(endpoint, msg);
                return Err(Error::Ipc(ipc::IpcError::NoSpace));
            }
        }
    }
    let total = msg.payload.len();
    let wanted = if out.payload_max == 0 { 0 } else { total };
    let too_big = wanted > out.payload_max && !out.truncate;
    let n = if too_big { 0 } else { core::cmp::min(wanted, out.payload_max) };
    let written = ipc.write(out.header_ptr, &msg.header.to_le_bytes())
        && out.sender_ptr.is_none_or(|ptr| ipc.write(ptr, &msg.sender_service_id.to_le_bytes()))
        && (n == 0 || ipc.write(out.payload_ptr, &msg.payload.as_slice()[..n]));
    if !written {
        if let Some(held) = placed.and_then(|slot| ipc.caps().take_held(slot).ok()) {
            msg.moved_cap = Some(held);
            msg.header.src = src;
        }
        let _ = ipc.router.requeue_front(endpoint, msg);
        return Ok(Outcome::Locked);
    }
    #[cfg(feature = "ipc_trace_ring")]
    if placed.is_some() && msg.header.len > 1024 {
        crate::ipc::trace::maybe_dump_capmove_big_recv("capmove-big-recv");
    }
    if too_big {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    if n != 0 {
        crate::ipc_stats::record_payload_copy(n);
    }
    Ok(Outcome::Done(n))
}

impl Context<'_> {
    /// The IPC view of this context: the same objects, the caller's record.
    pub(super) fn ipc(&self) -> Result<Ipc<'_>, Error> {
        let me = self.tasks.current_pid();
        let rec = crate::sched::record::get(me).ok_or(AddressSpaceError::InvalidHandle)?;
        Ok(Ipc {
            me,
            rec,
            router: self.router,
            waitsets: self.waitsets,
            fences: self.fences,
            timer: self.timer,
            timers: self.hart_timers,
            spaces: self.address_spaces,
            free: false,
        })
    }
}

/// Ends an IPC syscall's body under the kernel lock: a blocked caller's hart takes the next
/// task (the trap exit switches), or — nothing runnable — goes to its scheduler loop; before
/// that loop exists the caller wakes itself and the syscall re-runs (bring-up, one hart).
pub(super) fn locked(ctx: &mut Context<'_>, out: IpcResult) -> SysResult<usize> {
    match out? {
        Outcome::Done(value) => Ok(value),
        // The hart is given up at the trap's exit, once the return value is in the frame.
        Outcome::Yielded => {
            ctx.yield_after = true;
            Ok(0)
        }
        // The entry committed every page the body names before it ran: a page still not
        // usable is not the caller's to use.
        Outcome::Locked => Err(AddressSpaceError::BadUserMemory.into()),
        Outcome::Blocked(waits) => {
            wake_expired_blocked(ctx);
            if let Some(next) = ctx.scheduler.schedule_next() {
                ctx.tasks.set_current(next);
                return Err(Error::Reschedule);
            }
            let cur = ctx.tasks.current_pid();
            Err(park_hart_or_self_wake(ctx, cur, |ctx| {
                if let Ok(ipc) = ctx.ipc() {
                    waits.withdraw(&ipc);
                }
            }))
        }
    }
}

/// An IPC syscall's body.
pub(crate) type Body = fn(&Ipc<'_>, &Args) -> IpcResult;

/// The body of syscall `nr`, for the trap's entry before the kernel lock; `None`: not an IPC
/// syscall.
pub(crate) fn body(nr: usize) -> Option<Body> {
    use crate::syscall as nrs;
    let body: Body = match nr {
        nrs::SYSCALL_YIELD => sched_task::yield_now,
        nrs::SYSCALL_IPC_SEND_V1 => ipc_msg::send_v1,
        nrs::SYSCALL_IPC_RECV_V1 => ipc_msg::recv_v1,
        nrs::SYSCALL_IPC_RECV_V2 => |ipc, args| ipc_recv_v2::recv_v2(ipc, args.get(0)),
        nrs::SYSCALL_IPC_CALL_V1 => ipc_call::call,
        nrs::SYSCALL_IPC_REPLY_RECV_V1 => ipc_call::reply_recv,
        nrs::SYSCALL_WAITSET_WAIT => sync_objects::waitset_wait,
        nrs::SYSCALL_FENCE_SIGNAL => sync_objects::fence_signal,
        nrs::SYSCALL_FENCE_WAIT => sync_objects::fence_wait,
        // TASK-0330 P3: a mapping and a device's runs work on the caller's space and on the
        // object, each behind its own lock.
        nrs::SYSCALL_VM_MAP => vm_map::vm_map,
        nrs::SYSCALL_VM_UNMAP => vm_map::vm_unmap,
        nrs::SYSCALL_VMO_RUNS => {
            |ipc, args| page_in_asks_for_the_lock(vmo_runs::vmo_runs(ipc, args))
        }
        // TASK-0330 P4: bytes into and out of an object the caller holds for the call.
        nrs::SYSCALL_VMO_WRITE => {
            |ipc, args| page_in_asks_for_the_lock(vmo_copy::vmo_write(ipc, args))
        }
        nrs::SYSCALL_VMO_READ => {
            |ipc, args| page_in_asks_for_the_lock(vmo_copy::vmo_read(ipc, args))
        }
        // A second capability, and the close of one of any kind — the caller's table behind
        // its process's lock, each object behind its own (an endpoint's death included).
        nrs::SYSCALL_CAP_CLOSE => caps::cap_close,
        nrs::SYSCALL_CAP_CLONE => caps::cap_clone,
        // The timer table is behind its own lock; the console gathers a task's line under its.
        nrs::SYSCALL_TIMER_SET => sync_objects::timer_set,
        nrs::SYSCALL_TIMER_CANCEL => sync_objects::timer_cancel,
        nrs::SYSCALL_DEBUG_PUTC => |ipc, args| Ok(Outcome::Done(console_byte(ipc.me, args))),
        _ => return None,
    };
    Some(body)
}

/// `debug_putc`: one byte of `pid`'s console line, gathered per task and sent as a whole line
/// at its `\n` (TASK-0333 6f-3) — the line table's own lock, on either path.
pub(super) fn console_byte(pid: task::Pid, args: &Args) -> usize {
    let byte = (args.get(0) & 0xff) as u8;
    crate::hal::console_line::task_write(pid.as_raw(), &[byte]);
    byte as usize
}

/// Before the kernel lock nothing can park the caller on a swapped page of the object
/// (ADR-0088): the lock's path runs the syscall again and parks it there.
fn page_in_asks_for_the_lock(out: IpcResult) -> IpcResult {
    match out {
        Err(Error::PageIn { .. }) => Ok(Outcome::Locked),
        out => out,
    }
}
