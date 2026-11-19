// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The origin of a surface's change (RFC-0101 §8) — which layer of the present draws
//! the rows an app's present changed. The planner blurs a glass again only for a change below
//! it and ends a change at an opaque layer between its origin and the glass, so the origin must
//! be a layer that draws the change with no other layer drawing it above: several layers may
//! draw the same rows — the body and a region over it draw base rows, the desktop's bar strip
//! draws them again above the windows, a motion layer's pixels are its claimed regions' too, a
//! band's are its slices'. An origin BELOW another layer that draws the change would let that
//! layer hide its own change from the glasses above it.
//! A window's layers are one run of the frame and share its transform id (its own glasses never
//! blur for its own change): the origin is the TOPMOST of them that draws the change. The
//! desktop's layers lie below and above the windows: the change is named once for each layer
//! that draws it — and with more of them than a present has room for, its origin is unknown
//! (`0`: below every glass, behind no cover). A key the present does not hold is unknown to the
//! planner too, so a layer left out of a frame degrades by itself. Pure, host-TESTED; the keys
//! are the encoder's (`compositor::runtime::surface_layers_encode`, `nexus_gfx` `layer_wire`).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below
//! INVARIANTS: an origin is a layer that draws the change, or unknown — never a layer below
//!   another one of its surface that draws the same change.

use crate::layer_geometry::{pieces, Piece, StoreLayer, SurfaceLayers, MAX_PIECES};
use crate::motion_geometry::{region_owner, scroll_owner};
use crate::surface_presentation::SHELL_TOPBAR_H;
use nexus_display_proto::client_surface::{MATERIAL_GLASS, MATERIAL_OPAQUE};
use nexus_gfx::command::layer_wire::{
    layer_key, layer_key_kind, layer_key_owner, LAYER_KIND_BAR, LAYER_KIND_BODY, LAYER_KIND_REGION,
    LAYER_KIND_SLICE,
};

/// The key's owner of the desktop surface's layers (the desktop's key).
const DESKTOP_OWNER: u32 = crate::window_keys::DESKTOP_KEY as u32;

/// The first slice key index of the motion layers' own pixels (a scroll layer's slices use
/// `k * MAX_PIECES + j`).
pub(crate) const MOTION_KEY_BASE: u32 = 0x80;

/// The most layers of the desktop one change is named for beside its body; a change more
/// layers draw is of unknown origin.
const DESKTOP_DRAWERS: usize = 2;

/// A rectangle in surface pixels.
type Rect = (u32, u32, u32, u32);

/// What of a surface a present changed, in surface coordinates: base rows, or a store layer's
/// rows shown in a rectangle (a motion layer's rest, a scroll layer's viewport).
#[derive(Clone, Copy)]
pub(crate) enum Changed {
    Base(Rect),
    Store(StoreLayer, Rect),
}

/// The key of surface `owner`'s region `i`: the origin of a region that moved or came.
pub(crate) const fn region_key(owner: u32, i: usize) -> u32 {
    layer_key(owner, LAYER_KIND_REGION, i as u32)
}

/// Where `a` and `b` overlap.
fn cut(a: Rect, b: Rect) -> Option<Rect> {
    let (x0, y0) = (a.0.max(b.0), a.1.max(b.1));
    let x1 = a.0.saturating_add(a.2).min(b.0.saturating_add(b.2));
    let y1 = a.1.saturating_add(a.3).min(b.1.saturating_add(b.3));
    (x1 > x0 && y1 > y0).then(|| (x0, y0, x1 - x0, y1 - y0))
}

/// The origins of a change of surface `owner`: `each(key, rectangle)` in surface coordinates —
/// one for a window (the topmost layer that draws the change), one for each layer that draws it
/// for the desktop, or one of key `0` when those are more than a present has room for.
pub(crate) fn origins(
    layers: &SurfaceLayers,
    owner: u32,
    changed: Changed,
    mut each: impl FnMut(u32, Rect),
) {
    let tree = &layers.tree;
    let desktop = owner == DESKTOP_OWNER;
    let (first, all, claimed_by) = match changed {
        Changed::Base(r) => (layer_key(owner, LAYER_KIND_BODY, 0), r, None),
        Changed::Store(StoreLayer::Motion(k), r) => {
            (layer_key(owner, LAYER_KIND_SLICE, MOTION_KEY_BASE + k as u32), r, Some(k))
        }
        // A band's rows are its slices' alone, drawn one after the other: the last of them.
        Changed::Store(StoreLayer::Scroll(k), r) => {
            let mut out = [Piece::default(); MAX_PIECES];
            let n = tree.scrolls().get(k).map_or(0, |s| pieces(s, layers.offset(k), &mut out));
            let top = (k * MAX_PIECES + n.saturating_sub(1)) as u32;
            return each(layer_key(owner, LAYER_KIND_SLICE, top), r);
        }
    };
    // The regions drawing the same rows over their own back, lowest first: the unclaimed ones
    // sample the base, a motion layer's sample its pixels. The desktop's base is opaque
    // already: only its glass regions are layers.
    let regions = tree.regions().iter().enumerate().filter_map(|(i, l)| {
        let drawn = l.material == MATERIAL_GLASS || (!desktop && l.material == MATERIAL_OPAQUE);
        let rect = (u32::from(l.x), u32::from(l.y), u32::from(l.w), u32::from(l.h));
        (drawn && region_owner(tree, i) == claimed_by)
            .then(|| Some((region_key(owner, i), cut(all, rect)?)))
            .flatten()
    });
    if !desktop {
        return each(regions.last().map_or(first, |(key, _)| key), all);
    }
    // The bar strip draws the base's first rows again, above the windows.
    let bar = match changed {
        Changed::Base(_) => cut(all, (0, 0, u32::MAX, SHELL_TOPBAR_H)),
        Changed::Store(..) => None,
    };
    let mut above = [(0, (0, 0, 0, 0)); DESKTOP_DRAWERS];
    let mut n = 0;
    for drawer in bar.map(|r| (layer_key(owner, LAYER_KIND_BAR, 0), r)).into_iter().chain(regions) {
        match above.get_mut(n) {
            Some(slot) => *slot = drawer,
            None => return each(0, all),
        }
        n += 1;
    }
    each(first, all);
    for (key, rect) in &above[..n] {
        each(*key, *rect);
    }
}

/// The motion layer of surface `owner` the layer of `key` belongs to: its own pixels, a scroll
/// layer or a region it claims — whose damage is that motion's (`windowd: motion done`).
pub(crate) fn motion_of_key(layers: &SurfaceLayers, owner: u32, key: u32) -> Option<usize> {
    if u32::from(layer_key_owner(key)) != owner {
        return None;
    }
    let (tree, index) = (&layers.tree, key & 0xff);
    match layer_key_kind(key) {
        LAYER_KIND_SLICE if index >= MOTION_KEY_BASE => {
            Some((index - MOTION_KEY_BASE) as usize).filter(|k| *k < tree.motions().len())
        }
        LAYER_KIND_SLICE => scroll_owner(tree, index as usize / MAX_PIECES),
        LAYER_KIND_REGION => region_owner(tree, index as usize),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use nexus_display_proto::client_surface::LayerDesc;
    use nexus_display_proto::surface_layers::{MotionLayerDesc, ScrollLayerDesc, AXIS_VERTICAL};

    const WINDOW: u32 = 7;
    const WHOLE: Rect = (0, 0, u32::MAX, u32::MAX);

    fn region(x: u16, y: u16, w: u16, h: u16, material: u8) -> LayerDesc {
        LayerDesc { x, y, w, h, material, ..LayerDesc::default() }
    }

    fn with_regions(regions: &[LayerDesc]) -> SurfaceLayers {
        let mut layers = SurfaceLayers::EMPTY;
        layers.tree.regions[..regions.len()].copy_from_slice(regions);
        layers.tree.region_count = regions.len();
        layers
    }

    fn named(layers: &SurfaceLayers, owner: u32, changed: Changed) -> Vec<(u32, Rect)> {
        let mut out = Vec::new();
        origins(layers, owner, changed, |key, r| out.push((key, r)));
        out
    }

    const fn body(owner: u32) -> u32 {
        layer_key(owner, LAYER_KIND_BODY, 0)
    }

    #[test]
    fn a_window_s_base_rows_with_no_region_over_them_are_its_body_s() {
        let layers = with_regions(&[region(0, 0, 200, 600, MATERIAL_GLASS)]);
        let r = (400, 100, 50, 20);
        assert_eq!(named(&layers, WINDOW, Changed::Base(r)), [(body(WINDOW), r)]);
        assert_eq!(
            named(&SurfaceLayers::EMPTY, WINDOW, Changed::Base(WHOLE)),
            [(body(WINDOW), WHOLE)]
        );
    }

    /// A region draws its base rows over its own back: named for the body, the region — a glass
    /// back, an opaque pane — would hide its own change from every glass above the window.
    #[test]
    fn test_reject_naming_the_body_for_rows_a_region_of_the_window_draws_too() {
        let layers = with_regions(&[
            region(0, 0, 200, 600, MATERIAL_OPAQUE),
            region(100, 0, 300, 100, MATERIAL_GLASS),
            region(800, 0, 100, 100, MATERIAL_GLASS),
        ]);
        // Inside regions 0 and 1: the topmost of them, with the whole rectangle.
        let r = (150, 10, 20, 20);
        assert_eq!(named(&layers, WINDOW, Changed::Base(r)), [(region_key(WINDOW, 1), r)]);
        // Half inside region 0 alone.
        let r = (0, 300, 400, 10);
        assert_eq!(named(&layers, WINDOW, Changed::Base(r)), [(region_key(WINDOW, 0), r)]);
        // The whole surface: its topmost region.
        assert_eq!(named(&layers, WINDOW, Changed::Base(WHOLE)), [(region_key(WINDOW, 2), WHOLE)]);
    }

    #[test]
    fn a_motion_layer_s_rows_are_its_own_pixels_or_the_topmost_region_it_claims() {
        let mut layers = with_regions(&[
            region(0, 0, 200, 600, MATERIAL_OPAQUE),
            region(20, 30, 100, 200, MATERIAL_GLASS),
        ]);
        layers.tree.motions[0] =
            MotionLayerDesc { x: 20, y: 30, w: 100, h: 200, sy: 900, ..MotionLayerDesc::EMPTY };
        layers.tree.motion_count = 1;
        let rest = (20, 30, 100, 200);
        let own = layer_key(WINDOW, LAYER_KIND_SLICE, MOTION_KEY_BASE);
        // It claims nothing: region 1 samples the base, not the layer's pixels.
        let changed = Changed::Store(StoreLayer::Motion(0), rest);
        assert_eq!(named(&layers, WINDOW, changed), [(own, rest)]);
        assert_eq!(motion_of_key(&layers, WINDOW, own), Some(0));
        assert_eq!(motion_of_key(&layers, WINDOW, region_key(WINDOW, 1)), None);
        // And base rows there are region 1's, never the motion layer's.
        assert_eq!(named(&layers, WINDOW, Changed::Base(rest)), [(region_key(WINDOW, 1), rest)]);
        // It claims region 1: the region draws the layer's pixels over its back.
        layers.tree.motions[0].regions = 1 << 1;
        assert_eq!(named(&layers, WINDOW, changed), [(region_key(WINDOW, 1), rest)]);
        assert_eq!(motion_of_key(&layers, WINDOW, region_key(WINDOW, 1)), Some(0));
        assert_eq!(named(&layers, WINDOW, Changed::Base(rest)), [(region_key(WINDOW, 0), rest)]);
        // Another window's key, a body, a motion layer that is not there: no motion's.
        assert_eq!(motion_of_key(&layers, WINDOW + 1, own), None);
        assert_eq!(motion_of_key(&layers, WINDOW, body(WINDOW)), None);
        assert_eq!(motion_of_key(&layers, WINDOW, own + 1), None);
    }

    #[test]
    fn a_band_s_rows_are_its_last_slice_s() {
        let mut layers = SurfaceLayers::EMPTY;
        let band = ScrollLayerDesc {
            vx: 310,
            vy: 120,
            vw: 1080,
            vh: 700,
            axis: AXIS_VERTICAL,
            flags: 0,
            bx: 0,
            by: 869,
            strip_len: 1400,
            strips: 1,
            valid_start: 1000,
            valid_len: 1400,
        };
        layers.tree.scrolls[1] = band;
        layers.tree.scroll_count = 2;
        let viewport = (310, 120, 1080, 700);
        let changed = Changed::Store(StoreLayer::Scroll(1), viewport);
        let slice = |j: usize| layer_key(DESKTOP_OWNER, LAYER_KIND_SLICE, (MAX_PIECES + j) as u32);
        // Offset 0 is before the valid range: nothing of it is drawn, the key is the first's.
        let mut out = [Piece::default(); MAX_PIECES];
        let pieces_at = |layers: &SurfaceLayers, out: &mut [Piece; MAX_PIECES]| {
            pieces(&layers.tree.scrolls[1], layers.offset(1), out)
        };
        let n = pieces_at(&layers, &mut out);
        assert_eq!(
            named(&layers, DESKTOP_OWNER, changed),
            [(slice(n.saturating_sub(1)), viewport)]
        );
        assert_eq!(motion_of_key(&layers, DESKTOP_OWNER, slice(0)), None, "no motion claims it");
        layers.tree.motions[0] = MotionLayerDesc { scrolls: 1 << 1, ..MotionLayerDesc::EMPTY };
        layers.tree.motion_count = 1;
        assert_eq!(motion_of_key(&layers, DESKTOP_OWNER, slice(0)), Some(0));
    }

    /// The desktop's layers lie below and above the windows: a change of its bar rows is the
    /// body's below them and the bar strip's above them — named for the body alone, an opaque
    /// window between would hide the strip's change from a panel's glass.
    #[test]
    fn test_reject_naming_the_desktop_s_body_alone_for_rows_drawn_above_the_windows() {
        let layers = with_regions(&[region(1500, 40, 400, 500, MATERIAL_GLASS)]);
        let bar = layer_key(DESKTOP_OWNER, LAYER_KIND_BAR, 0);
        // The clock: bar rows.
        let r = (900, 8, 120, 20);
        assert_eq!(
            named(&layers, DESKTOP_OWNER, Changed::Base(r)),
            [(body(DESKTOP_OWNER), r), (bar, r)]
        );
        // Across the bar's edge: the strip draws its rows of it alone.
        let r = (900, 30, 120, 20);
        assert_eq!(
            named(&layers, DESKTOP_OWNER, Changed::Base(r)),
            [(body(DESKTOP_OWNER), r), (bar, (900, 30, 120, SHELL_TOPBAR_H - 30))]
        );
        // In the panel: the body's and the panel's, cut at the panel.
        let r = (1400, 100, 200, 50);
        assert_eq!(
            named(&layers, DESKTOP_OWNER, Changed::Base(r)),
            [(body(DESKTOP_OWNER), r), (region_key(DESKTOP_OWNER, 0), (1500, 100, 100, 50))]
        );
        // The wallpaper's part of the base, under no region: the body's alone.
        let r = (100, 500, 50, 50);
        assert_eq!(named(&layers, DESKTOP_OWNER, Changed::Base(r)), [(body(DESKTOP_OWNER), r)]);
        // An opaque region of the desktop is no layer: the base holds it.
        let plain = with_regions(&[region(0, 100, 400, 400, MATERIAL_OPAQUE)]);
        assert_eq!(named(&plain, DESKTOP_OWNER, Changed::Base(r)), [(body(DESKTOP_OWNER), r)]);
    }

    #[test]
    fn a_desktop_change_more_layers_draw_than_a_present_names_is_of_unknown_origin() {
        let layers = with_regions(&[
            region(0, 0, 100, 30, MATERIAL_GLASS),
            region(200, 0, 100, 30, MATERIAL_GLASS),
            region(400, 100, 100, 100, MATERIAL_GLASS),
        ]);
        // The bar and one pill beside the body: named apart.
        assert_eq!(named(&layers, DESKTOP_OWNER, Changed::Base((0, 0, 50, 10))).len(), 3);
        // The whole surface: the body, the bar, three regions — one rectangle, origin unknown.
        assert_eq!(named(&layers, DESKTOP_OWNER, Changed::Base(WHOLE)), [(0, WHOLE)]);
    }
}
