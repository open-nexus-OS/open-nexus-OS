// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Scheduling/task-lifecycle syscalls split out of the former
//! single-file api.rs: sys_task_qos (born-at-class QoS policy + audit),
//! sys_sched (affinity/shares), sys_yield, sys_exit/sys_wait, sys_as_self,
//! sys_spawn/sys_task_resume/sys_spawn_last_error and selftest_sched_op.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: neuron host tests + QEMU marker gates (just test-os / ci-os-smp)
//! ADR: docs/adr/0016-kernel-libs-architecture.md

use super::*;

// Typed decoders for seL4-style Decode→Check→Execute

#[derive(Copy, Clone)]
pub(super) struct SpawnArgsTyped {
    entry_pc: VirtAddr,
    stack_sp: Option<VirtAddr>,
    as_handle: Option<AsHandle>,
    bootstrap_slot: SlotIndex,
    global_pointer: usize,
}

impl SpawnArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        let entry_pc =
            VirtAddr::instr_aligned(args.get(0)).ok_or(AddressSpaceError::InvalidArgs)?;
        let stack_raw = args.get(1);
        let stack_sp = if stack_raw == 0 {
            None
        } else {
            // Accept a normal stack pointer (not necessarily page aligned), but require a canonical VA.
            Some(VirtAddr::new(stack_raw).ok_or(AddressSpaceError::InvalidArgs)?)
        };
        let raw_handle = args.get(2) as u32;
        let as_handle = AsHandle::from_raw(raw_handle);
        let bootstrap_slot = SlotIndex::decode(args.get(3));
        let global_pointer = args.get(4);
        Ok(Self { entry_pc, stack_sp, as_handle, bootstrap_slot, global_pointer })
    }

    #[inline]
    fn check(&self) -> Result<(), Error> {
        if self.as_handle.is_some() && self.stack_sp.is_none() {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        if self.as_handle.is_none() && self.stack_sp.is_some() {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        Ok(())
    }
}

pub(super) const TASK_QOS_OP_GET_SELF: usize = 0;
pub(super) const TASK_QOS_OP_SET: usize = 1;

#[derive(Copy, Clone)]
pub(super) struct TaskQosArgsTyped {
    op: usize,
    target: task::Pid,
    qos_raw: u8,
}

impl TaskQosArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        let raw_target = args.get(1);
        if raw_target > u32::MAX as usize {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        let raw_qos = args.get(2);
        if raw_qos > u8::MAX as usize {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        Ok(Self {
            op: args.get(0),
            target: task::Pid::from_raw(raw_target as u32),
            qos_raw: raw_qos as u8,
        })
    }
}

pub(super) fn service_id_from_name(name: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325u64;
    for &b in name {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3u64);
    }
    h
}

pub(super) fn caller_has_qos_admin(ctx: &Context<'_>) -> bool {
    // Declarative admin list: execd/policyd (B4 recipes) + init (P1: applies
    // the service_topology::affinity_for placement at resume time).
    const QOS_ADMINS: &[&[u8]] = &[b"execd", b"policyd", b"init-lite", b"nexus-init"];
    let sid = ctx.tasks.current_service_id();
    let mut i = 0;
    while i < QOS_ADMINS.len() {
        if sid == service_id_from_name(QOS_ADMINS[i]) {
            return true;
        }
        i += 1;
    }
    false
}

/// Born-at-class QoS — the production soft-real-time boot policy, declared in ONE place and applied
/// at task creation (`exec_v2`). The critical path to the first frame + mouse is created `Interactive`
/// so it strictly preempts the core + background → the first frame is deterministic and fast, never
/// starved by round-robin contention (that was the measured 617/1164/2133ms non-determinism). policyd
/// gates the MMIO grants; gpud/windowd are the display; inputd/hidrawd the mouse. Background services
/// not needed before the first frame are `Idle` so they can never starve it. Everything else is `Normal`.
pub(super) fn initial_qos_for(service_id: u64) -> QosClass {
    // DEFERRED: raising the display/input path (policyd/gpud/windowd/inputd/hidrawd) to Interactive
    // requires the display present loop to BLOCK reactively FIRST (boot P1) — otherwise an Interactive
    // busy-spin (gpud/windowd self-pace their present loop by polling) strictly starves the Normal
    // core + Idle background and the boot HANGS (observed at "windowd: gl procedural cursor on"). Until
    // P1 lands, keep the critical path at Normal and only DEMOTE background so it can't starve it.
    //
    // netstackd is NOT spawned Idle: spawning a background service Idle is a
    // chicken-and-egg trap — on the strict-priority scheduler the Normal
    // busy-spinners (gpud/windowd present-poll, hidrawd yield-spin) keep the
    // Normal queue perpetually non-empty, so the Idle queue NEVER runs and the
    // service never reaches its entry → never emits `ready` → never self-lowers.
    // (Boot-log-proven: netstackd/dsoftbusd/touchd/selftest-client all missed
    // `ready`; the headless ladder stalled on `netstackd: ready`.) netstackd is
    // resumed BEFORE the display drivers and self-lowers to Idle right after
    // emitting `ready` (os_entry: emit_ready_marker → task_qos_set_self(Idle)),
    // so its brief Normal bring-up window cannot starve the first frame.
    //
    // selftest-client is ALSO not spawned Idle: its `os_lite::run()` raises
    // itself to Interactive for the proof ladder (mod.rs), but it could never
    // REACH `run()` while parked at Idle (the same starvation trap) → the whole
    // selftest ladder, including the OTA phase that emits `bundlemgrd: slot a
    // active`, never ran (headless OTA-phase stall). Spawned Normal it is
    // scheduled (resumed before the display drivers), reaches `run()`, and
    // manages its own QoS from there.
    //
    // dsoftbusd/touchd keep the (latent) Idle trap until each is confirmed to
    // self-lower after a bounded bring-up — tracked with the boot P1
    // busy-spinner fix (they don't gate the proof ladder).
    if service_id == service_id_from_name(b"dsoftbusd")
        || service_id == service_id_from_name(b"touchd")
    {
        QosClass::Idle
    } else {
        QosClass::Normal
    }
}

pub(super) fn qos_label(qos: QosClass) -> &'static str {
    match qos {
        QosClass::Idle => "idle",
        QosClass::Normal => "normal",
        QosClass::Interactive => "interactive",
        QosClass::PerfBurst => "perfburst",
    }
}

pub(super) fn qos_audit(
    ctx: &Context<'_>,
    target: task::Pid,
    from: QosClass,
    to: QosClass,
    decision: &'static str,
    reason: &'static str,
) {
    // RFC-0068: QoS audit is a runtime event → DEBUG (off by default; NEXUS_LOG=qos=debug).
    log_debug!(
        target: "qos",
        "QOS-AUDIT decision={} reason={} caller_sid=0x{:016x} caller_pid={} target_pid={} from={} to={}",
        decision,
        reason,
        ctx.tasks.current_service_id(),
        ctx.tasks.current_pid().as_raw(),
        target.as_raw(),
        qos_label(from),
        qos_label(to)
    );
}

pub(super) fn qos_audit_reject_simple(ctx: &Context<'_>, target: task::Pid, reason: &'static str) {
    // RFC-0068: QoS audit is a runtime event → DEBUG (off by default; NEXUS_LOG=qos=debug).
    log_debug!(
        target: "qos",
        "QOS-AUDIT decision=deny reason={} caller_sid=0x{:016x} caller_pid={} target_pid={}",
        reason,
        ctx.tasks.current_service_id(),
        ctx.tasks.current_pid().as_raw(),
        target.as_raw()
    );
}

pub(super) fn sys_task_qos(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = TaskQosArgsTyped::decode(args)?;
    match typed.op {
        TASK_QOS_OP_GET_SELF => Ok(ctx.tasks.current_task().qos() as usize),
        TASK_QOS_OP_SET => {
            let qos = match QosClass::from_u8(typed.qos_raw) {
                Some(v) => v,
                None => {
                    qos_audit_reject_simple(ctx, typed.target, "invalid_qos_class");
                    return Err(AddressSpaceError::InvalidArgs.into());
                }
            };
            let target = typed.target;
            let current = ctx.tasks.current_pid();
            let target_qos = match ctx.tasks.task(target) {
                Some(task) => task.qos(),
                None => {
                    qos_audit_reject_simple(ctx, target, "invalid_target_pid");
                    return Err(AddressSpaceError::InvalidArgs.into());
                }
            };
            let privileged = caller_has_qos_admin(ctx);

            if target != current && !privileged {
                qos_audit(ctx, target, target_qos, qos, "deny", "unauthorized_other_pid");
                return Err(Error::Capability(CapError::PermissionDenied));
            }
            if (qos as u8) > (target_qos as u8) && !privileged {
                qos_audit(ctx, target, target_qos, qos, "deny", "unauthorized_escalation");
                return Err(Error::Capability(CapError::PermissionDenied));
            }

            if matches!(ctx.scheduler.set_task_qos(target, qos), SetQosOutcome::QueueFull) {
                qos_audit(ctx, target, target_qos, qos, "deny", "scheduler_queue_full");
                return Err(Error::Ipc(ipc::IpcError::QueueFull));
            }
            let task = ctx.tasks.task_mut(target).ok_or(AddressSpaceError::InvalidArgs)?;
            task.set_qos(qos);
            qos_audit(ctx, target, target_qos, qos, "allow", "applied");
            Ok(0)
        }
        _ => Err(AddressSpaceError::InvalidArgs.into()),
    }
}

/// B (TASK-0042): scheduling-attribute ops. args = (op, target_pid, value).
/// target 0 = self. Cross-task ops require the QoS-admin capability (same
/// privilege model as sys_task_qos).
pub(super) const SCHED_OP_GET_AFFINITY: usize = 0;
pub(super) const SCHED_OP_SET_AFFINITY: usize = 1;
pub(super) const SCHED_OP_GET_SHARES: usize = 2;
pub(super) const SCHED_OP_SET_SHARES: usize = 3;

/// Selftest entry into the REAL sched handler (proves the ABI clamp and
/// validation logic itself, not a reimplementation).
pub(crate) fn selftest_sched_op(
    ctx: &mut Context<'_>,
    op: usize,
    target: usize,
    value: usize,
) -> SysResult<usize> {
    let args = Args::new([op, target, value, 0, 0, 0]);
    sys_sched(ctx, &args)
}

pub(super) fn sys_sched(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    // OP 4 (P0, declarative budgets SSOT in core/trap/budgets.rs): emit the
    // boot-end BKL budget gate line. Read-only; callable late by the selftest
    // ladder so the report COVERS the service bring-up contention window.
    // OP 5 (P0 two-window): log the bring-up burst maxima, then reset the
    // accounting so the boot-end gate judges the steady-state window.
    if args.get(0) == 5 {
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            let (_, wait_us, hold_ms, nr, b) = crate::trap::budgets::budget_report();
            log_info!(
                target: "smp",
                "KINIT: bkl bring-up burst max_wait={}us max_hold={}ms nr={} gt10ms={}",
                wait_us,
                hold_ms,
                nr,
                b[3]
            );
            crate::trap::budgets::reset();
        }
        return Ok(0);
    }
    if args.get(0) == 4 {
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            let (ok, wait_us, hold_ms, nr, b) = crate::trap::budgets::budget_report();
            log_info!(
                target: "smp",
                "KINIT: bkl histogram le100us={} le1ms={} le10ms={} gt10ms={}",
                b[0],
                b[1],
                b[2],
                b[3]
            );
            if ok {
                log_info!(
                    target: "smp",
                    "KSELFTEST: bkl budget ok (max_wait={}us max_hold={}ms nr={})",
                    wait_us,
                    hold_ms,
                    nr
                );
            } else {
                log_error!(
                    target: "smp",
                    "KSELFTEST: bkl budget FAIL max_wait={}us max_hold={}ms nr={}",
                    wait_us,
                    hold_ms,
                    nr
                );
            }
        }
        return Ok(0);
    }
    let op = args.get(0);
    let raw_target = args.get(1);
    if raw_target > u32::MAX as usize {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    let current = ctx.tasks.current_pid();
    let target = if raw_target == 0 { current } else { task::Pid::from_raw(raw_target as u32) };
    if target != current && !caller_has_qos_admin(ctx) {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    let value = args.get(2);
    match op {
        SCHED_OP_GET_AFFINITY => {
            let task = ctx.tasks.task(target).ok_or(AddressSpaceError::InvalidArgs)?;
            Ok(task.affinity_mask() as usize)
        }
        SCHED_OP_SET_AFFINITY => {
            let mask = crate::task::validate_affinity_mask(value, crate::smp::cpu_online_mask())
                .map_err(|_| Error::AddressSpace(AddressSpaceError::InvalidArgs))?;
            let online = crate::smp::cpu_online_mask();
            let home = {
                let task = ctx.tasks.task_mut(target).ok_or(AddressSpaceError::InvalidArgs)?;
                task.set_affinity_mask(mask);
                // Re-clamp the home CPU; takes effect on the next wake/dispatch.
                crate::task::clamp_home_to_affinity(mask, task.home_cpu(), online)
            };
            ctx.tasks.set_home_cpu(target, home);
            Ok(0)
        }
        SCHED_OP_GET_SHARES => {
            let task = ctx.tasks.task(target).ok_or(AddressSpaceError::InvalidArgs)?;
            Ok(task.shares() as usize)
        }
        SCHED_OP_SET_SHARES => {
            // Deterministic clamp to [1, 1000] (plan contract).
            let shares = value.clamp(1, 1000) as u16;
            let task = ctx.tasks.task_mut(target).ok_or(AddressSpaceError::InvalidArgs)?;
            task.set_shares(shares);
            Ok(shares as usize)
        }
        _ => Err(AddressSpaceError::InvalidArgs.into()),
    }
}

pub(super) fn sys_spawn_last_error(ctx: &mut Context<'_>, _args: &Args) -> SysResult<usize> {
    let pid = ctx.tasks.current_pid();
    let reason =
        ctx.tasks.take_last_spawn_fail_reason(pid).unwrap_or(crate::task::SpawnFailReason::Unknown);
    Ok(reason.as_u8() as usize)
}

pub(super) fn sys_yield(ctx: &mut Context<'_>, _args: &Args) -> SysResult<usize> {
    crate::liveness::bump();
    // Honor pending IPC deadlines at every scheduling transition, not just at
    // recv/send blocks. A task that polls via `yield_()` would otherwise keep the
    // scheduler perpetually runnable, so a peer blocked on a timed recv (windowd's
    // 120Hz pacer, gpud's spin-blur re-present) is never woken at its deadline —
    // the cooperative analogue of a missed timer IRQ.
    wake_expired_blocked(ctx);
    ctx.scheduler.yield_current();
    if let Some(next) = ctx.scheduler.schedule_next() {
        ctx.tasks.set_current(next);
        if let Some(task) = ctx.tasks.task(next) {
            #[cfg(feature = "debug_uart")]
            {
                use core::fmt::Write as _;
                let mut w = crate::uart::raw_writer();
                let _ = write!(w, "YIELD-I: next pid={} sepc=0x{:x}\n", next, task.frame().sepc);
            }
            #[cfg(not(feature = "debug_uart"))]
            let _ = task; // silence unused when debug UART is disabled
        }
        Ok(next.as_index())
    } else {
        Ok(ctx.tasks.current_pid().as_index())
    }
}

pub(super) fn sys_exit(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let status = args.get(0) as i32;
    let exiting = ctx.tasks.current_pid();
    // RFC-0005 lifecycle: close endpoints owned by this task and wake any blocked peers.
    let waiters = ctx.router.close_endpoints_for_owner(exiting.as_raw());
    ctx.router.remove_waiter_from_all(exiting.as_raw());
    ctx.tasks.exit_current(status);
    for pid in waiters {
        observe_wake_outcome(ctx.tasks.wake(task::Pid::from_raw(pid), ctx.scheduler));
    }
    ctx.tasks.wake_parent_waiter(exiting, ctx.scheduler);
    ctx.scheduler.finish_current();
    if let Some(next) = ctx.scheduler.schedule_next() {
        ctx.tasks.set_current(next);
        if let Some(task) = ctx.tasks.task(next) {
            #[cfg(not(feature = "selftest_no_satp"))]
            {
                if let Some(handle) = task.address_space() {
                    ctx.address_spaces.activate(handle)?;
                }
            }
            #[cfg(feature = "selftest_no_satp")]
            let _ = task;
        }
    } else {
        ctx.tasks.set_current(task::Pid::KERNEL);
    }
    Err(Error::TaskExit)
}

pub(super) fn sys_wait(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let raw_pid = args.get(0) as i32;
    let target = if raw_pid <= 0 { None } else { Some(task::Pid::from_raw(raw_pid as u32)) };
    loop {
        match ctx.tasks.reap_child(target, ctx.address_spaces) {
            Ok((pid, status)) => {
                if let Some(task) = ctx.tasks.task_mut(ctx.tasks.current_pid()) {
                    task.frame_mut().x[11] = status as usize;
                }
                return Ok(pid.as_index());
            }
            Err(task::WaitError::WouldBlock) => {
                let cur = ctx.tasks.current_pid();
                ctx.tasks.block_current(BlockReason::WaitChild { target }, ctx.scheduler);
                if let Some(next) = ctx.scheduler.schedule_next() {
                    ctx.tasks.set_current(next);
                    return Err(Error::Reschedule);
                }
                observe_wake_outcome(ctx.tasks.wake(cur, ctx.scheduler));
                return Err(Error::Reschedule);
            }
            Err(err) => return Err(Error::from(err)),
        }
    }
}

// CRITICAL: ABI surface for userspace spawn. Keep Decode→Check→Execute and rights checks stable.
/// C (Phase C): the caller's own AS handle (raw, non-zero). Threads are
/// spawned by passing this handle to SYSCALL_SPAWN — the new task shares the
/// address space (and gets an EMPTY capability table: compute-only threads
/// by construction, per the TASK-0276 policy).
pub(super) fn sys_as_self(ctx: &mut Context<'_>, _args: &Args) -> SysResult<usize> {
    let pid = ctx.tasks.current_pid();
    let handle = ctx
        .tasks
        .task(pid)
        .and_then(|t| t.address_space())
        .ok_or(Error::AddressSpace(AddressSpaceError::InvalidHandle))?;
    Ok(handle.to_raw() as usize)
}

pub(super) fn sys_spawn(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = SpawnArgsTyped::decode(args)?;
    let sp_raw = typed.stack_sp.map(|v| v.raw()).unwrap_or(0);
    let as_raw = typed.as_handle.map(|h| h.to_raw()).unwrap_or(0);
    // RFC-0068: process-spawn is a runtime event → DEBUG (off by default; NEXUS_LOG=sys=debug).
    log_debug!(
        target: "sys",
        "SPAWN: entry=0x{:x} sp=0x{:x} as={} slot={} gp=0x{:x}",
        typed.entry_pc.raw(),
        sp_raw,
        as_raw,
        typed.bootstrap_slot.0,
        typed.global_pointer
    );
    typed.check()?;

    let parent = ctx.tasks.current_pid();
    let pid = match ctx.tasks.spawn(
        parent,
        typed.entry_pc,
        typed.stack_sp,
        typed.as_handle,
        typed.global_pointer,
        typed.bootstrap_slot,
        ctx.scheduler,
        ctx.router,
        ctx.address_spaces,
    ) {
        Ok(pid) => pid,
        Err(err) => {
            let reason = crate::task::spawn_fail_reason(&err);
            ctx.tasks.set_last_spawn_fail_reason(parent, reason);
            return Err(err.into());
        }
    };

    // Kernel-spawned tasks (selftest, init-lite) must run immediately.
    // Userspace-spawned services stay suspended until the parent resumes them.
    if typed.as_handle.is_none_or(|h| h.to_raw() == 0) {
        let _ = ctx.tasks.resume_task(pid, ctx.scheduler);
    }

    Ok(pid.as_index())
}

pub(super) fn sys_task_resume(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let pid_raw = args.get(0) as u32;
    let pid = task::Pid::from_raw(pid_raw);
    match ctx.tasks.resume_task(pid, ctx.scheduler) {
        task::ResumeOutcome::Resumed => Ok(0),
        task::ResumeOutcome::TaskNotFound => Err(Error::InvalidTarget),
        task::ResumeOutcome::NotSuspended => Err(Error::InvalidTarget),
        task::ResumeOutcome::EnqueueRejected => Err(Error::RunQueueFull),
    }
}
