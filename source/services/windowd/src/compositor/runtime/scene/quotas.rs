// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The frame's layer budget split across its surfaces (RFC-0109 failure model): the drawn
//! windows' bodies and the desktop's base and bar first, then the declared layers max-min fair.
//! Split out of `scene.rs` (the module-size ratchet).

use super::*;
use crate::layer_quota::fair_shares;

/// Each surface's share of the frame's layers (RFC-0109 failure model): the drawn windows' bodies
/// and the desktop's base and bar first, then the declared layers max-min fair (`fair_shares`) —
/// index `slot` a window, `snaps.len()` the desktop — into `quotas`. The caret is its surface's
/// last layer and counts in that surface's demand (`caret`: its window slot, `Some(None)` the
/// desktop) — left out, every share was exactly the declared layers and the caret the one dropped.
/// Returns the layers the frame draws: the bodies and every share.
pub(super) fn layer_quotas(
    (snaps, order, culled): (&[AppSceneSnap], &[WindowId], &[bool]),
    desktop: Option<&super::surface_layers_encode::LayerSnap>,
    caret: Option<Option<u16>>,
    (demand, share, quotas): (&mut Vec<usize>, &mut Vec<usize>, &mut Vec<LayerQuota>),
) -> usize {
    let n = snaps.len() + 1;
    demand.clear();
    demand.resize(n, 0);
    share.clear();
    share.resize(n, 0);
    let mut bodies = if desktop.is_some() { 2 } else { 0 };
    for wid in order {
        let WindowId::App(slot) = *wid else { continue };
        let Some(sn) = snaps.get(usize::from(slot)) else { continue };
        if culled.get(usize::from(slot)).is_some_and(|c| *c) {
            continue;
        }
        bodies += usize::from(sn.glass.is_some() || sn.solid.is_some());
        if sn.solid.is_none() {
            let holds_caret = caret == Some(Some(slot));
            demand[usize::from(slot)] =
                sn.surface.map_or(0, |l| l.layers.demand()) + usize::from(holds_caret);
        }
    }
    let desktop_caret = usize::from(caret == Some(None));
    demand[n - 1] = desktop.map_or(0, |l| l.layers.demand() + desktop_caret);
    fair_shares(crate::layer_quota::FRAME_LAYERS.saturating_sub(bodies), demand, share);
    quotas.clear();
    quotas.extend(share.iter().zip(demand.iter()).map(|(s, d)| LayerQuota::new(*s).covering(*d)));
    bodies + share.iter().sum::<usize>()
}
