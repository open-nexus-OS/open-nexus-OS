// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: GPU memory, charged to its client, as the memory authority knows it (TASK-0333 6e-4,
//! RFC-0100 §3, ADR-0078/0086). The kernel's per-space figures (`mm_stats`) count what a process
//! maps; what the GPU holds FOR a process — the framebuffer gpud lent the compositor, a client's
//! context buffers — is in gpud's hands and charged by gpud alone (`OP_GPU_CHARGE`: an absolute
//! figure per client, replacing the last; zero frees the row). This table is the authority's
//! copy: by kernel service id (the key gpud speaks), bounded like gpud's own, a ninth client
//! refused by name. It completes the picture a kill is judged on; the join to a process's pid
//! comes with app surfaces of their own (layer 8), when a client is an app.
//! OWNERS: @runtime @gpu
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `tests/contract.rs` (`test_reject_gpu_charge_from_anyone_but_gpud`, a charge
//!   replaces and zero frees); the display lanes' `memd: gpu charge (client=windowd`

/// Clients the table holds at once (gpud's own bound).
pub const MAX_CLIENTS: usize = 8;

/// A ninth client: refused, nothing counted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Full;

/// Every GPU client's charge, by kernel service id.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GpuCharges {
    rows: [Option<(u64, u64)>; MAX_CLIENTS],
}

impl GpuCharges {
    #[must_use]
    pub const fn new() -> Self {
        Self { rows: [None; MAX_CLIENTS] }
    }

    /// `client` costs `bytes` now (absolute; zero frees the row). Answers the table's total.
    pub fn set(&mut self, client: u64, bytes: u64) -> Result<u64, Full> {
        let known = self.rows.iter().position(|r| r.is_some_and(|(c, _)| c == client));
        match (known, bytes) {
            (Some(i), 0) => self.rows[i] = None,
            (Some(i), b) => self.rows[i] = Some((client, b)),
            (None, 0) => {}
            (None, b) => {
                let free = self.rows.iter().position(Option::is_none).ok_or(Full)?;
                self.rows[free] = Some((client, b));
            }
        }
        Ok(self.total())
    }

    /// What `client` costs.
    #[must_use]
    pub fn of(&self, client: u64) -> u64 {
        self.rows.iter().flatten().find(|(c, _)| *c == client).map_or(0, |(_, b)| *b)
    }

    /// What every client costs together.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.rows.iter().flatten().map(|(_, b)| *b).fold(0, u64::saturating_add)
    }

    /// Clients charged.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.iter().flatten().count()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_charge_replaces_zero_frees_and_a_ninth_is_refused() {
        let mut g = GpuCharges::new();
        assert_eq!(g.set(7, 100), Ok(100));
        assert_eq!(g.set(7, 60), Ok(60), "absolute, not a delta");
        assert_eq!(g.set(8, 40), Ok(100));
        assert_eq!(g.of(7), 60);
        assert_eq!(g.set(7, 0), Ok(40));
        assert_eq!(g.len(), 1);
        assert_eq!(g.set(9, 0), Ok(40), "a zero for nobody is nothing");
        for id in 10..10 + MAX_CLIENTS as u64 - 1 {
            assert!(g.set(id, 1).is_ok());
        }
        assert_eq!(g.len(), MAX_CLIENTS);
        assert_eq!(g.set(99, 1), Err(Full));
        assert_eq!(g.of(99), 0);
    }
}
