// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the kills execd ORDERED (TASK-0333 6d, ADR-0082): a child execd ended with
//! `task_kill` — on the lifecycle authority's order or in its own probe — dies with the
//! kernel's reason `Killed`, the same reason a fail-fast kill carries. The reaper must tell
//! the two apart: an ordered kill is the system's decision, not a crash, so no crash report,
//! no minidump, no crash line. A bounded table of ordered pids, written before the order and
//! consumed by the reap; a kill whose pid fell off the table is reported as a crash (an
//! over-full table makes noise, never silence). Pure and host-tested, like `identity`: the
//! OS glue (`os_lite`) is RISC-V-only.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `SELFTEST: task kill ok` on every lane (the probe's ordered kill reaped as
//!   `Killed` without a crash report); host: `cargo test -p execd orders`

use core::sync::atomic::{AtomicU32, Ordering};

/// Kills in flight at once. Eight is more than the app bands ever hold (6e's pressure kills are
/// one at a time); a ninth order is still executed, only reported as a crash.
const SLOTS: usize = 8;
const NONE: u32 = 0;

static ORDERED: [AtomicU32; SLOTS] = [const { AtomicU32::new(NONE) }; SLOTS];

/// Records `pid` as about to be killed on order. Returns `false` when the table is full (the
/// kill proceeds; its reap will read as a crash).
pub(crate) fn note_ordered_kill(pid: u32) -> bool {
    if pid == NONE {
        return false;
    }
    ORDERED
        .iter()
        .any(|slot| slot.compare_exchange(NONE, pid, Ordering::AcqRel, Ordering::Relaxed).is_ok())
}

/// Whether `pid` was killed on order; the entry is consumed.
pub(crate) fn take_ordered_kill(pid: u32) -> bool {
    pid != NONE
        && ORDERED.iter().any(|slot| {
            slot.compare_exchange(pid, NONE, Ordering::AcqRel, Ordering::Relaxed).is_ok()
        })
}

/// Withdraws a recorded order (the kill syscall itself was refused).
pub(crate) fn forget_ordered_kill(pid: u32) {
    let _ = take_ordered_kill(pid);
}

/// The two gates of a lifecycle order (`OP_FREEZE`/`OP_THAW`/`OP_KILL`, RFC-0111 §5): the
/// sender is the lifecycle authority (its KERNEL sender id) and the pid is a live child of this
/// service. Both fail-closed; nothing in the frame can open either.
pub(crate) fn order_allowed(sender_service_id: u64, authority: u64, live_child: bool) -> bool {
    sender_service_id == authority && live_child
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_order_is_consumed_once() {
        assert!(note_ordered_kill(4242));
        assert!(take_ordered_kill(4242));
        assert!(!take_ordered_kill(4242));
    }

    #[test]
    fn test_reject_unordered_pid_and_zero() {
        assert!(!take_ordered_kill(999_999));
        assert!(!note_ordered_kill(0));
        assert!(!take_ordered_kill(0));
    }

    #[test]
    fn test_reject_order_from_a_stranger_or_for_a_stranger() {
        let (authority, stranger) = (0x1111u64, 0x2222u64);
        assert!(order_allowed(authority, authority, true));
        assert!(!order_allowed(stranger, authority, true), "only the authority orders");
        assert!(!order_allowed(authority, authority, false), "only a live child is ordered");
        assert!(!order_allowed(stranger, authority, false));
    }

    #[test]
    fn the_table_is_bounded_and_noisy_when_full() {
        let base = 70_000u32;
        let mut noted = 0;
        for i in 0..(SLOTS as u32 + 2) {
            if note_ordered_kill(base + i) {
                noted += 1;
            }
        }
        assert!(noted <= SLOTS);
        for i in 0..(SLOTS as u32 + 2) {
            forget_ordered_kill(base + i);
        }
    }
}
