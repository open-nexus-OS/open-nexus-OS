// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the bound of execd's child table (TASK-0333 6f-3, found by board cycle 76). execd
//! keeps one row per child it spawned: a live child's row is what every lifecycle order checks
//! (`orders::order_allowed` — freeze, thaw, squeeze, kill) and what tells the reap that a dead
//! child was an app; an exited child's row answers a late `OP_WAIT_PID` from the cache. The table
//! is bounded for the exited rows only: before one more child is tracked, the OLDEST EXITED row
//! goes; a live row never does — a running app execd forgot could never be frozen, squeezed or
//! killed again, and its death would never reach the lifecycle authority (cycle 76: settings,
//! started before the harness's fifteen probe children, pushed out while it ran). With every row
//! alive the table grows past the bound while that many children live; the kernel's task table
//! bounds them. Pure and host-tested; the table itself is `os_lite`'s (RISC-V-only).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host `cargo test -p execd children` (`test_reject_evicting_a_live_child`);
//!   every lane's lifecycle orders

/// Exited rows the table keeps at most (live rows are never counted out).
pub(crate) const MAX_TRACKED: usize = 32;

/// The row to drop before one more child is tracked, given each row's exited state in table
/// order (oldest first): the oldest exited row once the table holds `MAX_TRACKED` rows; `None`
/// while there is room, or when every row is a live child.
pub(crate) fn evict_index(exited: impl ExactSizeIterator<Item = bool>) -> Option<usize> {
    if exited.len() < MAX_TRACKED {
        return None;
    }
    exited.enumerate().find(|(_, gone)| *gone).map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(exited: &[bool]) -> Option<usize> {
        evict_index(exited.iter().copied())
    }

    #[test]
    fn a_table_with_room_drops_nothing() {
        assert_eq!(rows(&[]), None);
        assert_eq!(rows(&[true; MAX_TRACKED - 1]), None, "one row short of the bound");
    }

    #[test]
    fn a_full_table_drops_its_oldest_exited_row() {
        let mut table = [false; MAX_TRACKED];
        table[3] = true;
        table[9] = true;
        assert_eq!(rows(&table), Some(3), "the oldest exited, not the oldest row");
        assert_eq!(rows(&[true; MAX_TRACKED]), Some(0));
        let mut over = vec![false; MAX_TRACKED + 4];
        over[MAX_TRACKED + 2] = true;
        assert_eq!(rows(&over), Some(MAX_TRACKED + 2), "a grown table sheds exited rows first");
    }

    /// The bug of board cycle 76: a running app pushed out of the table by newer children, every
    /// order for it refused afterwards. A live row is never the one that goes.
    #[test]
    fn test_reject_evicting_a_live_child() {
        assert_eq!(rows(&[false; MAX_TRACKED]), None, "every row alive: the table grows");
        assert_eq!(rows(&[false; MAX_TRACKED + 7]), None);
        let mut table = [false; MAX_TRACKED];
        table[MAX_TRACKED - 1] = true;
        assert_eq!(rows(&table), Some(MAX_TRACKED - 1), "the only exited row, however new");
    }
}
