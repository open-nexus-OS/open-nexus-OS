// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the sched syscall's time-right operation (RFC-0120 §Syscall surface, TASK-0363 P3):
//! `(TIME_RIGHT_OP, op, a, b, c, d)` — claim the root (init, once), read what a right holds,
//! derive a subset into a new slot (with a grain: the smallest part ever taken from it, what
//! bounds the slots an untrusted holder can fill), bind a share of it to a task, unbind, revoke.
//! A right is handed to a child as its capability (`cap_rules::manage_travels`). Authority is
//! the `TimeRight` capability in the caller's table with `MANAGE` (its payload is a generation-
//! checked id: a copy outliving a revoke names nothing); binding another task's reservation
//! takes the same standing as setting its class (the QoS admins), since a reservation places
//! the task on a hart. Every number is checked by the contract (`nexus_sched::TimeRights`)
//! before it changes anything; a refusal changes nothing.
//! OWNERS: @kernel-sched-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `selftest/smp_sched/time_right.rs` (the `KSELFTEST: test_reject_time_right_*`
//!   lines, through the syscall table); the contract in `nexus-sched/tests/time_right.rs`
//! RFC: docs/rfcs/RFC-0120-time-is-a-right-a-share-derived-as-a-subset-a-reservation-admitted-per-hart-revoked-in-one-step.md

use super::*;
use crate::sched::reserve;
use nexus_sched::{RightId, TimeRightError, TimeRightOp, TIME_RIGHT_OP};

/// A refusal of the contract as the syscall's error.
fn refused(e: TimeRightError) -> Error {
    match e {
        TimeRightError::Revoked | TimeRightError::Root | TimeRightError::BeyondHeld => {
            Error::Capability(CapError::PermissionDenied)
        }
        TimeRightError::BeyondCapacity | TimeRightError::Full => {
            Error::Capability(CapError::NoSpace)
        }
        _ => Error::AddressSpace(AddressSpaceError::InvalidArgs),
    }
}

/// The right slot `slot` of the caller's table names (`MANAGE` required).
fn right_at(ctx: &mut Context<'_>, slot: usize) -> Result<RightId, Error> {
    let cap = ctx.tasks.current_caps_mut().get(slot)?;
    if !cap.rights.contains(Rights::MANAGE) {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    match cap.kind {
        CapabilityKind::TimeRight(raw) => Ok(RightId::from_raw(raw)),
        _ => Err(Error::Capability(CapError::InvalidSlot)),
    }
}

/// `right` into a new slot of the caller's table.
fn install(ctx: &mut Context<'_>, right: RightId) -> SysResult<usize> {
    let cap =
        Capability { kind: CapabilityKind::TimeRight(right.to_raw()), rights: Rights::MANAGE };
    let caps = ctx.tasks.current_caps_mut();
    Held::plain(cap).and_then(|held| caps.allocate(held)).map_err(Error::Capability)
}

/// The task a bind or an unbind names: the caller (0), or another one for a QoS admin.
fn target_of(ctx: &Context<'_>, raw: usize) -> Result<task::Pid, Error> {
    let current = ctx.tasks.current_pid();
    if raw == 0 {
        return Ok(current);
    }
    let pid = u32::try_from(raw).map_err(|_| AddressSpaceError::InvalidArgs)?;
    let target = task::Pid::from_raw(pid);
    if target != current && !super::sched_task::caller_has_qos_admin(ctx) {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    ctx.tasks.task(target).ok_or(AddressSpaceError::InvalidArgs)?;
    Ok(target)
}

/// The time-right operation, if `args` is one (`None`: another sched op).
pub(super) fn time_right_op(ctx: &mut Context<'_>, args: &Args) -> Option<SysResult<usize>> {
    if args.get(0) != TIME_RIGHT_OP {
        return None;
    }
    Some(run(ctx, args))
}

fn run(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let op = TimeRightOp::from_raw(args.get(1)).ok_or(AddressSpaceError::InvalidArgs)?;
    let (a, b, c, d) = (args.get(2), args.get(3), args.get(4), args.get(5));
    let share = |raw: usize| u32::try_from(raw).map_err(|_| AddressSpaceError::InvalidArgs);
    match op {
        TimeRightOp::ClaimRoot => {
            if !super::sched_task::caller_is_init(ctx) {
                return Err(Error::Capability(CapError::PermissionDenied));
            }
            let root = reserve::claim_root().ok_or(CapError::PermissionDenied)?;
            install(ctx, root)
        }
        TimeRightOp::Held => {
            let (total, own) =
                reserve::held(right_at(ctx, a)?).ok_or(CapError::PermissionDenied)?;
            Ok(((total as usize) << 32) | own as usize)
        }
        TimeRightOp::Derive => {
            let (parent, grain) = (right_at(ctx, a)?, share(c)?);
            let child = reserve::derive(parent, share(b)?, grain).map_err(refused)?;
            install(ctx, child).inspect_err(|_| {
                let _ = reserve::revoke(child);
            })
        }
        TimeRightOp::Bind => {
            let right = right_at(ctx, a)?;
            let target = target_of(ctx, b)?;
            let task = ctx.tasks.task(target).ok_or(AddressSpaceError::InvalidArgs)?;
            let online = crate::smp::cpu_online();
            let allowed = task.affinity().intersection(online);
            let home = task.home_cpu().as_index();
            // Its home first, then the other harts it may run on.
            let harts = core::iter::once(home)
                .filter(|&h| allowed.contains(h))
                .chain(allowed.iter().filter(move |&h| h != home));
            let hart = reserve::bind(target, right, share(c)?, d as u64, harts).map_err(refused)?;
            ctx.tasks.set_home_cpu(target, crate::types::CpuId::from_raw(hart as u16));
            Ok(hart)
        }
        TimeRightOp::Unbind => {
            let right = right_at(ctx, a)?;
            let target = target_of(ctx, b)?;
            reserve::unbind(target, right).map_err(refused)?;
            Ok(0)
        }
        TimeRightOp::Revoke => {
            reserve::revoke(right_at(ctx, a)?).map_err(refused)?;
            Ok(0)
        }
    }
}
