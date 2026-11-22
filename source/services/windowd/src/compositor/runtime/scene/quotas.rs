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
    // The desktop's bar strip and its body's one slot (none for an empty ink, RFC-0121).
    let mut bodies =
        desktop.map_or(0, |l| 1 + usize::from(crate::body_ink::drawn(l.layers.tree.ink())));
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
    // The body's pieces past its slot (RFC-0121) are the desktop's demand too: drawn only when
    // its share covers all of it (`body_pieces`), else the one full-screen piece.
    demand[n - 1] = desktop.map_or(0, |l| {
        l.layers.demand() + desktop_caret + crate::body_ink::extra(l.layers.tree.ink(), l.base)
    });
    fair_shares(crate::layer_quota::FRAME_LAYERS.saturating_sub(bodies), demand, share);
    quotas.clear();
    quotas.extend(share.iter().zip(demand.iter()).map(|(s, d)| LayerQuota::new(*s).covering(*d)));
    bodies + share.iter().sum::<usize>()
}

/// The desktop body's pieces this frame draws (RFC-0121), into `out`: the ink's pieces when the
/// desktop's share (`quota`, before its declared layers spend it) covers its whole demand and
/// takes the pieces past the body slot — all or none; otherwise the one full-screen piece of
/// before, and the share stays its declared layers'. A declared empty ink draws no piece.
pub(super) fn body_pieces(
    l: &super::surface_layers_encode::LayerSnap,
    quota: &mut LayerQuota,
    out: &mut crate::body_ink::Pieces,
) -> usize {
    let ink = l.layers.tree.ink();
    let n = crate::body_ink::pieces(ink, l.base, out);
    if n <= 1 || quota.take_all(n - 1) {
        return n;
    }
    crate::body_ink::pieces(None, l.base, out)
}
