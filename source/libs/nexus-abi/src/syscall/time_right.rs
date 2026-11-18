// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: time as a right (RFC-0120, TASK-0363 P3) — the sched syscall's time-right
//! operation. A time right is a capability: a share of the machine's time (ppm of one CPU),
//! derived from the root init holds, only ever as a subset of what its holder still has. A
//! holder binds a part of it to a task as a reservation — a budget every period, run before
//! everything fair on the CPU that admitted it — and revokes a right with everything derived
//! from it in one step. Binding another task takes the standing that sets its class (init,
//! execd, policyd); a task binds itself with nothing but the right. The kernel checks every
//! number and refuses rather than clamps.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `KSELFTEST: test_reject_time_right_*` (the kernel's side of every call here);
//!   `init: time rights` on every boot; the rules are host-tested in `nexus-sched`
//! RFC: docs/rfcs/RFC-0120-time-is-a-right-a-share-derived-as-a-subset-a-reservation-admitted-per-hart-revoked-in-one-step.md

use super::*;
use nexus_sched::{TimeRightOp, TIME_RIGHT_OP};

const SYSCALL_SCHED: usize = 48;

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn op(op: TimeRightOp, a: usize, b: usize, c: usize, d: usize) -> SysResult<usize> {
    // SAFETY: plain syscall; the kernel validates every argument.
    let raw = unsafe { ecall6(SYSCALL_SCHED, TIME_RIGHT_OP, op as usize, a, b, c, d) };
    decode_syscall(raw)
}

/// Claims the machine's root time right: init, once. Everything else is derived from it.
#[must_use = "the root right is what every other right is derived from"]
pub fn claim_root() -> SysResult<Cap> {
    op(TimeRightOp::ClaimRoot, 0, 0, 0, 0).map(|slot| slot as Cap)
}

/// What `right` was given and what of it it still holds itself — the rest is derived or bound
/// — in ppm of one CPU: `(total, own)`.
#[must_use = "a right's share is what a derive or a bind is sized from"]
pub fn held(right: Cap) -> SysResult<(u32, u32)> {
    op(TimeRightOp::Held, right as usize, 0, 0, 0)
        .map(|word| ((word as u64 >> 32) as u32, word as u32))
}

/// Derives a right of `share_ppm` from `right` into a new slot: a subset of what `right` still
/// holds itself, refused beyond it.
#[must_use = "the derived right is a capability to keep or hand on"]
pub fn derive(right: Cap, share_ppm: u32) -> SysResult<Cap> {
    op(TimeRightOp::Derive, right as usize, share_ppm as usize, 0, 0).map(|slot| slot as Cap)
}

/// Binds `share_ppm` of `right` to the caller as a reservation of `period_ns`: that share of
/// every period runs before everything fair. Returns the CPU that admitted it — where the
/// caller runs from now on. A reservation it held is ended first.
#[must_use = "a refused reservation runs fair"]
pub fn bind(right: Cap, share_ppm: u32, period_ns: u64) -> SysResult<usize> {
    op(TimeRightOp::Bind, right as usize, 0, share_ppm as usize, period_ns as usize)
}

/// [`bind`] for task `pid` (the standing that sets its class: init, execd, policyd).
#[must_use = "a refused reservation runs fair"]
pub fn bind_for(right: Cap, pid: u32, share_ppm: u32, period_ns: u64) -> SysResult<usize> {
    op(TimeRightOp::Bind, right as usize, pid as usize, share_ppm as usize, period_ns as usize)
}

/// Ends the caller's reservation bound from `right`: its share is the right's again.
#[must_use = "sched outcomes must be handled"]
pub fn unbind(right: Cap) -> SysResult<()> {
    op(TimeRightOp::Unbind, right as usize, 0, 0, 0).map(|_| ())
}

/// [`unbind`] for task `pid`.
#[must_use = "sched outcomes must be handled"]
pub fn unbind_for(right: Cap, pid: u32) -> SysResult<()> {
    op(TimeRightOp::Unbind, right as usize, pid as usize, 0, 0).map(|_| ())
}

/// Revokes `right` and every right derived from it, in one step: their reservations end, their
/// shares are the parent's again. The root is never revoked.
#[must_use = "sched outcomes must be handled"]
pub fn revoke(right: Cap) -> SysResult<()> {
    op(TimeRightOp::Revoke, right as usize, 0, 0, 0).map(|_| ())
}
