// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `SYSCALL_MM_STATS` (61, RFC-0098 C4, TASK-0286 P5) — the memory record
//! (`crate::accounting::MmStats`): the frame pool, the page tables, the objects, and
//! the caller's own residency. No authority: nothing in it names another task.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Unstable (versioned record)
//! TEST_COVERAGE: `accounting` host tests (layout, the short-buffer reject);
//!   `metricsd: mm snapshot ok (…)` reads it on every QEMU boot

use super::*;
use crate::accounting::{check_out_len, MM_STATS_BYTES};

/// Args: (out_ptr, out_len). Returns the bytes written.
pub(super) fn sys_mm_stats(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let (out_ptr, out_len) = (args.get(0), args.get(1));
    check_out_len(out_len).map_err(|_| AddressSpaceError::InvalidArgs)?;
    ensure_user_slice(out_ptr, MM_STATS_BYTES)?;
    let own = ctx.tasks.current_task().address_space();
    let (stats, _) = crate::mm::usage::snapshot(ctx.address_spaces, own);
    let mut record = [0u8; MM_STATS_BYTES];
    let written = stats.encode(&mut record).map_err(|_| AddressSpaceError::InvalidArgs)?;
    // SAFETY: `out_ptr..out_ptr + MM_STATS_BYTES` was checked to be a user slice.
    unsafe { ptr::copy_nonoverlapping(record.as_ptr(), out_ptr as *mut u8, written) };
    Ok(written)
}
