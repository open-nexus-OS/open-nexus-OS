// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: bytes into and out of a memory object by its capability — `vmo_write` (6) and
//! `vmo_read` (47) — without the kernel lock (TASK-0330 P4). One body serves both entries
//! (`ipc_ctx::body` before the lock, the syscall table under it). What the kernel lock kept
//! apart, the object's own counts keep apart now: the caller holds the object for the call
//! (`pinned`, so the id names it until the last byte), and the copy is counted at the object
//! like a mapping (`vmo::mapping::begin`) — taken in the table hold that refuses a sealed
//! object a writer and an object unlocked for purging anyone, and read by a seal, an unlock
//! and a destroy in theirs. So no frame the copy moves bytes through is sealed, purged or
//! freed under it; a squeeze takes only an object no capability outside its parked process
//! names. The caller's side is translated, never dereferenced (`mm::user_copy`): a page of its
//! buffer that is not ready is filled as its fault would fill it (`Ipc::fill`), and what that
//! refuses asks for the lock — the call runs again whole, the bytes moved so far are moved
//! again.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `KSELFTEST: vmo copy ok (…)` on every QEMU boot (a copy in flight refuses a
//!   seal, an unlock and a destroy; a sealed and an unlocked object refuse the copy);
//!   `KSELFTEST: userptr guard ok`; the object's doors on the host (`cap::tests_maps`); every
//!   lane's exec, atlas and surface paths write and read through here
//! INVARIANTS: no lock is held while bytes move; the caller's memory is reached through its
//!   page table's translation only; between two stretches the hart reads its shootdown mailbox
//! ADR: docs/adr/0103-a-fault-holds-its-own-address-space-and-nothing-else-a-shootdown-reaches-the-harts-that-hold-the-space.md

use super::ipc_ctx::{locked, Ipc, IpcResult, Outcome};
use super::*;

/// One stretch of a copy: the caller's pages are translated, the bytes moved, the hart's
/// shootdown mailbox read — an unmap on another hart waits for this much of a copy at most.
const STRETCH: usize = 4 * PAGE_SIZE;

/// What a read hands out for a page of a lazy object nobody touched yet.
static ZEROS: [u8; PAGE_SIZE] = [0; PAGE_SIZE];

/// Which way the bytes go.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Way {
    /// From the caller's buffer into the object.
    Write,
    /// Out of the object into the caller's buffer.
    Read,
}

impl Way {
    /// What the copy needs of the caller's pages.
    fn access(self) -> UserAccess {
        match self {
            Self::Write => UserAccess::Read,
            Self::Read => UserAccess::Write,
        }
    }

    fn fill(self) -> crate::lazy::Access {
        match self {
            Self::Write => crate::lazy::Access::Read,
            Self::Read => crate::lazy::Access::Write,
        }
    }

    fn by(self) -> &'static str {
        match self {
            Self::Write => "write",
            Self::Read => "read",
        }
    }
}

/// Args: (vmo_slot, offset, user_ptr, len).
#[derive(Copy, Clone)]
struct CopyArgs {
    slot: SlotIndex,
    offset: usize,
    user: usize,
    len: usize,
}

impl CopyArgs {
    fn decode(args: &Args) -> Self {
        Self {
            slot: SlotIndex::decode(args.get(0)),
            offset: args.get(1),
            user: args.get(2),
            len: args.get(3),
        }
    }
}

fn denied() -> Error {
    Error::Capability(CapError::PermissionDenied)
}

/// `SYSCALL_VMO_WRITE` (6) under the kernel lock (the syscall table: a kernel-mode task, a
/// caller an order waits for, the re-run of a body that asked for it).
pub(super) fn sys_vmo_write(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    under_the_lock(ctx, args, Way::Write)
}

/// `SYSCALL_VMO_READ` (47) under the kernel lock, as `sys_vmo_write`.
pub(super) fn sys_vmo_read(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    under_the_lock(ctx, args, Way::Read)
}

/// `SYSCALL_VMO_WRITE` (6): a bounded copy from the caller's buffer into the object. Requires
/// `Rights::MAP` on a writable capability; refused for a sealed object (ADR-0080) and for one
/// unlocked for purging (RFC-0100 §4). A lazy object's pages are committed as they are
/// written (ADR-0077).
pub(super) fn vmo_write(ipc: &Ipc<'_>, args: &Args) -> IpcResult {
    copy(ipc, args, Way::Write)
}

/// `SYSCALL_VMO_READ` (47): the mirror of `vmo_write` — out of the object into the caller's
/// buffer (the compositor's damage blit reads an app's surface this way, ADR-0042). A page of
/// a lazy object nobody touched reads as zeros and stays uncommitted.
pub(super) fn vmo_read(ipc: &Ipc<'_>, args: &Args) -> IpcResult {
    copy(ipc, args, Way::Read)
}

/// The table's entry: the body, and — a page of the caller's buffer the free fill would not
/// make ready — the range filled here, where a refusal is named, a squeezed page inflated and
/// a swapped one waited for, then the body once more.
fn under_the_lock(ctx: &mut Context<'_>, args: &Args, way: Way) -> SysResult<usize> {
    let mut out = copy(&ctx.ipc()?, args, way);
    if matches!(out, Ok(Outcome::Locked)) {
        let typed = CopyArgs::decode(args);
        ensure_user_range(ctx, typed.user, typed.len, way.access())?;
        out = copy(&ctx.ipc()?, args, way);
    }
    locked(ctx, out)
}

/// The body of both entries.
fn copy(ipc: &Ipc<'_>, args: &Args, way: Way) -> IpcResult {
    let typed = CopyArgs::decode(args);
    // Held for the call: the id below names this object until the last byte is moved.
    let pinned = super::pinned::pin(&ipc.caps(), typed.slot.0, Rights::MAP, way.by())?;
    let Some((id, object_len, false)) = pinned.object() else {
        return Err(denied());
    };
    if typed.offset.checked_add(typed.len).is_none_or(|end| end > object_len) {
        return Err(denied());
    }
    // Counted at the object for the copy's length, in the hold that judges its state: a sealed
    // object has no writer, an unlocked one is untouchable — and from here to the drop no
    // seal, no unlock and no destroy finds the object free of the copy.
    let reaching = crate::mm::vmo::mapping::begin(id, way == Way::Write).map_err(|_| denied())?;
    // RFC-0004: a pointer outside the user range is named without touching it.
    ensure_user_slice(typed.user, typed.len)?;
    #[cfg(feature = "debug_uart")]
    {
        use core::fmt::Write as _;
        let mut u = crate::uart::raw_writer();
        let _ = write!(
            u,
            "VMO-COPY {} slot=0x{:x} id={} off=0x{:x} len=0x{:x} user=0x{:x}\n",
            way.by(),
            typed.slot.0,
            id,
            typed.offset,
            typed.len,
            typed.user
        );
    }
    let pieces = crate::mm::vmo::span(id, typed.offset, typed.len, way == Way::Write)
        .map_err(|e| span_error(e, denied()))?;
    let mut at = typed.user;
    for (pa, piece) in pieces {
        if way == Way::Write && pa.is_none() {
            // A page the span did not commit is not the object's to write.
            return Err(denied());
        }
        let mut done = 0;
        while done < piece {
            let n = (piece - done).min(match pa {
                Some(_) => STRETCH,
                None => PAGE_SIZE,
            });
            let frame = pa.map(|pa| crate::phys::phys_to_virt(pa + done));
            let moved = stretch(ipc, way, at, frame, n)
                || (ipc.fill(at, n, way.fill()) && stretch(ipc, way, at, frame, n));
            if !moved {
                return Ok(Outcome::Locked);
            }
            let _ = crate::smp::tlb::poll_mailbox(crate::smp::cpu_current_id());
            at += n;
            done += n;
        }
    }
    if way == Way::Write && typed.len != 0 {
        riscv::asm::fence_i();
    }
    // The count first, then the capability: was it the object's last, it is judged there.
    drop(reaching);
    drop(pinned);
    Ok(Outcome::Done(typed.len))
}

/// Moves `n` bytes between the caller's memory at `at` and the object's frame bytes at `frame`
/// (`None`: a page of a lazy object nobody touched — a read's zeros). `false`: a page of the
/// caller's is not ready.
fn stretch(ipc: &Ipc<'_>, way: Way, at: usize, frame: Option<usize>, n: usize) -> bool {
    match (way, frame) {
        // SAFETY: `frame..frame + n` are bytes of the object's frames through the direct map —
        // one piece of its span, alive while the caller holds the object and is counted at it.
        (Way::Write, Some(frame)) => unsafe { ipc.read_raw(at, frame as *mut u8, n) },
        // SAFETY: as above, read only.
        (Way::Read, Some(frame)) => unsafe { ipc.write_raw(at, frame as *const u8, n) },
        (Way::Read, None) => ZEROS.get(..n).is_some_and(|zeros| ipc.write(at, zeros)),
        (Way::Write, None) => false,
    }
}
