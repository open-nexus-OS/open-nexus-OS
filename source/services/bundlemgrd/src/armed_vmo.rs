// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! CONTEXT: the VMOs senders armed for their next VMO op (`OP_ARM_VMO`, TASK-0324 P7-d),
//! keyed by the KERNEL sender identity — never by anything in a payload. Bounded, host-tested.
//! OWNERS: @runtime @security
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below (`cargo test -p bundlemgrd`)

/// Senders that may hold an armed VMO at once (the spawner, execd, packagefsd, the harness).
pub(crate) const CAPACITY: usize = 8;

/// One armed VMO per sender: `(sender_service_id, vmo cap slot)`; `slot == 0` = free.
pub(crate) struct ArmedVmos {
    rows: [(u64, u32); CAPACITY],
}

/// What an ARM did with the table.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Armed {
    /// Stored; nothing to release.
    Stored,
    /// Stored; the sender's earlier VMO is returned for the caller to release.
    Replaced(u32),
    /// The table is full: the VMO is returned for the caller to release (nothing stored).
    Full(u32),
}

impl ArmedVmos {
    pub(crate) const fn new() -> Self {
        Self { rows: [(0, 0); CAPACITY] }
    }

    /// Arms `vmo` for `sender`.
    pub(crate) fn arm(&mut self, sender: u64, vmo: u32) -> Armed {
        if let Some(row) = self.rows.iter_mut().find(|r| r.1 != 0 && r.0 == sender) {
            let old = row.1;
            row.1 = vmo;
            return Armed::Replaced(old);
        }
        match self.rows.iter_mut().find(|r| r.1 == 0) {
            Some(row) => {
                *row = (sender, vmo);
                Armed::Stored
            }
            None => Armed::Full(vmo),
        }
    }

    /// Takes the VMO `sender` armed — only its own: another sender's VMO is never served.
    pub(crate) fn take(&mut self, sender: u64) -> Option<u32> {
        let row = self.rows.iter_mut().find(|r| r.1 != 0 && r.0 == sender)?;
        let vmo = row.1;
        *row = (0, 0);
        Some(vmo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arm_then_take_serves_the_sender_once() {
        let mut t = ArmedVmos::new();
        assert_eq!(t.arm(7, 40), Armed::Stored);
        assert_eq!(t.take(7), Some(40));
        assert_eq!(t.take(7), None, "consumed");
    }

    /// The VMO a sender armed is never handed to a request from another sender.
    #[test]
    fn test_reject_take_by_another_sender() {
        let mut t = ArmedVmos::new();
        assert_eq!(t.arm(7, 40), Armed::Stored);
        assert_eq!(t.take(8), None);
        assert_eq!(t.take(7), Some(40));
    }

    #[test]
    fn arm_replaces_and_returns_the_old_vmo() {
        let mut t = ArmedVmos::new();
        assert_eq!(t.arm(7, 40), Armed::Stored);
        assert_eq!(t.arm(7, 41), Armed::Replaced(40));
        assert_eq!(t.take(7), Some(41));
    }

    /// Bounded: the ninth sender is refused with its VMO handed back, nothing stored.
    #[test]
    fn test_reject_arm_beyond_capacity() {
        let mut t = ArmedVmos::new();
        for s in 1..=CAPACITY as u64 {
            assert_eq!(t.arm(s, 40 + s as u32), Armed::Stored);
        }
        assert_eq!(t.arm(99, 77), Armed::Full(77));
        assert_eq!(t.take(99), None);
    }
}
