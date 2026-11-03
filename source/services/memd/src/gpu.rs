// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: GPU memory, charged to its client, as the memory authority knows it (TASK-0333 6e-4,
//! RFC-0100 §3, ADR-0078/0086). The kernel's per-space figures (`mm_stats`) count what a process
//! maps; what the GPU holds FOR a process — the framebuffer gpud lent the compositor, a client's
//! context buffers, an app's window surfaces — is in gpud's hands and charged by gpud alone
//! (`OP_GPU_CHARGE`: an absolute figure per client, replacing the last; zero frees the row). This
//! table is the authority's copy: by kernel service id (the key gpud speaks), one row per client
//! charged, as many as the clients (TASK-0352 P1, ADR-0093: no constant — the heap bounds it, and
//! a row the heap refuses is dropped by name). It completes the picture a kill is judged on; the
//! join to a process's pid comes with the apps' surfaces in their footprint (P2).
//! OWNERS: @runtime @gpu
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `tests/contract.rs` (`test_reject_gpu_charge_from_anyone_but_gpud`, a charge
//!   replaces and zero frees); the display lanes' `memd: gpu charge (client=windowd`

use alloc::vec::Vec;

/// The heap refused the row: nothing counted for this client (the charge is named, not lost in
/// silence — the next charge tries again).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoHeap;

/// Every GPU client's charge, by kernel service id.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GpuCharges {
    rows: Vec<(u64, u64)>,
}

impl GpuCharges {
    #[must_use]
    pub const fn new() -> Self {
        Self { rows: Vec::new() }
    }

    /// `client` costs `bytes` now (absolute; zero frees the row). Answers the table's total.
    pub fn set(&mut self, client: u64, bytes: u64) -> Result<u64, NoHeap> {
        let known = self.rows.iter().position(|(c, _)| *c == client);
        match (known, bytes) {
            (Some(i), 0) => {
                self.rows.swap_remove(i);
            }
            (Some(i), b) => self.rows[i] = (client, b),
            (None, 0) => {}
            (None, b) => {
                self.rows.try_reserve(1).map_err(|_| NoHeap)?;
                self.rows.push((client, b));
            }
        }
        Ok(self.total())
    }

    /// What `client` costs.
    #[must_use]
    pub fn of(&self, client: u64) -> u64 {
        self.rows.iter().find(|(c, _)| *c == client).map_or(0, |(_, b)| *b)
    }

    /// What every client costs together.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.rows.iter().map(|(_, b)| *b).fold(0, u64::saturating_add)
    }

    /// Clients charged.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_charge_replaces_zero_frees_and_the_ninth_client_is_charged_too() {
        let mut g = GpuCharges::new();
        assert_eq!(g.set(7, 100), Ok(100));
        assert_eq!(g.set(7, 60), Ok(60), "absolute, not a delta");
        assert_eq!(g.set(8, 40), Ok(100));
        assert_eq!(g.of(7), 60);
        assert_eq!(g.set(7, 0), Ok(40));
        assert_eq!(g.len(), 1);
        assert_eq!(g.set(9, 0), Ok(40), "a zero for nobody is nothing");
        // TASK-0352 P1 (ADR-0093): eight was gpud's old bound — forty apps' surfaces are forty rows.
        for id in 10..50u64 {
            assert_eq!(g.set(id, 1), Ok(40 + id - 9));
        }
        assert_eq!(g.len(), 41);
        assert_eq!(g.of(49), 1);
        assert_eq!(g.set(49, 0), Ok(79));
        assert_eq!(g.len(), 40, "a freed row leaves no hole");
    }
}
