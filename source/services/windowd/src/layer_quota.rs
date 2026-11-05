// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the frame's layer bound shared out among the surfaces it draws (RFC-0109 failure
//! model): max-min fair shares of the planner's set, and one surface's share as its encode
//! spends it — a surface over the bound loses its own last layers, never another window's.
//! Split out of `layer_geometry.rs` (module-size ratchet). Host-tested here; the OS-only scene
//! builder (`compositor::runtime::scene`) shares the frame out and each encode spends its share.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `mod tests` below

/// Max-min fair shares of the frame's layers (RFC-0109 failure model): `demand[i]` is what
/// surface `i` declares, `rest` what the planner's set holds beside the bodies. When everything
/// fits each surface keeps its demand; otherwise the smaller demands stay whole and the rest is
/// split evenly among the larger — a surface over the bound loses its own last layers, never
/// another window's.
pub(crate) fn fair_shares(rest: usize, demand: &[usize], share: &mut [usize]) {
    let n = demand.len().min(share.len());
    // `usize::MAX` marks a surface whose share is still open.
    share[..n].fill(usize::MAX);
    let (mut left, mut open) = (rest, n);
    while open > 0 {
        let even = left / open;
        let smallest = (0..n)
            .filter(|&i| share[i] == usize::MAX)
            .min_by_key(|&i| demand[i])
            .filter(|&i| demand[i] <= even);
        let Some(i) = smallest else {
            // Every open demand exceeds the even split: each takes it, the remainder in order.
            let mut extra = left - even * open;
            for s in share[..n].iter_mut().filter(|s| **s == usize::MAX) {
                *s = even + usize::from(extra > 0);
                extra = extra.saturating_sub(1);
            }
            return;
        };
        share[i] = demand[i];
        left -= demand[i];
        open -= 1;
    }
}

/// One surface's share of the frame's layers as its encode spends it: a layer past the share is
/// not composed, and counted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LayerQuota {
    left: usize,
    dropped: usize,
}

impl LayerQuota {
    pub(crate) const fn new(share: usize) -> Self {
        Self { left: share, dropped: 0 }
    }

    /// Sets one layer of the share aside BEFORE the surface spends it, for a layer drawn over
    /// the surface (the caret): a tight frame drops the surface's own last layer, never that one.
    /// `false` (not counted) when the share is empty — the surface is not drawn as layers.
    pub(crate) fn set_aside(&mut self) -> bool {
        if self.left == 0 {
            return false;
        }
        self.left -= 1;
        true
    }

    /// Whether one more layer fits the share (counted as dropped when it does not).
    pub(crate) fn take(&mut self) -> bool {
        if self.left == 0 {
            self.dropped += 1;
            return false;
        }
        self.left -= 1;
        true
    }

    /// The layers the surface lost to its share this frame.
    pub(crate) const fn dropped(&self) -> usize {
        self.dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A heavy surface beside light ones keeps every layer while the frame holds them all.
    #[test]
    fn every_declared_layer_is_composed_while_the_frame_holds_them() {
        let mut share = [0; 5];
        fair_shares(58, &[32, 3, 3, 3, 0], &mut share);
        assert_eq!(share, [32, 3, 3, 3, 0]);
    }

    /// RFC-0109: past the planner's set the greediest surfaces lose their own last layers; a
    /// surface declaring little keeps everything, and the frame stays inside the bound.
    #[test]
    fn test_reject_a_surface_crowding_out_another_window() {
        let mut share = [0; 5];
        fair_shares(58, &[32, 32, 32, 3, 3], &mut share);
        assert_eq!(&share[3..], &[3, 3], "the light windows keep theirs");
        assert_eq!(share[..3].iter().sum::<usize>(), 52);
        assert!(share[..3].iter().all(|&s| s == 17 || s == 18), "split evenly: {share:?}");
        let mut quota = LayerQuota::new(share[0]);
        let kept = (0..32).filter(|_| quota.take()).count();
        assert_eq!((kept, quota.dropped()), (share[0], 32 - share[0]));
    }

    /// RFC-0113 §5: the caret's layer set aside first is kept when the frame is tight — the
    /// surface loses its own last layer instead; an empty share sets nothing aside.
    #[test]
    fn a_layer_set_aside_survives_a_tight_share() {
        let mut quota = LayerQuota::new(3);
        assert!(quota.set_aside(), "the caret's layer");
        assert!(quota.take() && quota.take(), "two of the surface's layers fit");
        assert!(!quota.take(), "its third is the one dropped");
        assert_eq!(quota.dropped(), 1);
        let mut empty = LayerQuota::new(0);
        assert!(!empty.set_aside());
        assert_eq!(empty.dropped(), 0, "an undrawn surface counts no drop");
    }
}
