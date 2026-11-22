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

/// The layers one frame composes — the compositor's budget, shared out among its surfaces
/// below; the wire's number (`nexus_display_proto::FRAME_LAYERS`), which the executor's passes
/// are sized for. It was 64, one IPC frame's worth: twenty windows lost the launcher to it
/// (`board--2026-11-18T11-44-54`).
pub(crate) const FRAME_LAYERS: usize = nexus_display_proto::FRAME_LAYERS;
// One bound of a frame's bytes: what the scene serializes to is what a present carries.
const _: () =
    assert!(nexus_display_proto::PRESENT_BODY_MAX == nexus_gfx::command::buffer::MAX_SERIALIZED);

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
    /// The share covers the surface's whole demand: no layer of it is dropped this frame.
    whole: bool,
}

impl LayerQuota {
    pub(crate) const fn new(share: usize) -> Self {
        Self { left: share, dropped: 0, whole: false }
    }

    /// The share against the surface's `demand` (its pieces counted): whether every layer fits.
    pub(crate) const fn covering(self, demand: usize) -> Self {
        Self { whole: self.left >= demand, ..self }
    }

    /// Whether nothing of the surface is dropped this frame — a motion layer's pieces are drawn
    /// around its region only then, so the region they leave to is drawn too (all or none).
    pub(crate) const fn whole(&self) -> bool {
        self.whole
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

    /// Takes `n` layers at once when the share covers the surface's whole demand and holds them
    /// (all or none, RFC-0121: the desktop body's pieces); `false` takes nothing and counts
    /// nothing dropped — the caller draws its fallback in the slot it keeps.
    pub(crate) fn take_all(&mut self, n: usize) -> bool {
        if !self.whole || self.left < n {
            return false;
        }
        self.left -= n;
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

    /// A frame at its budget, serialized by the scene's own encoder — every layer with its
    /// glass and shadow, a transform for every layer and every window, the damage rectangles
    /// — is a present the wire carries (the serializer refused twenty-one windows at 8000
    /// bytes, `board--2026-11-18T13-29-53`), and two of them in flight lie in gpud's request
    /// queue of 32 frames beside the pointer's traffic.
    #[test]
    fn a_frame_at_its_budget_is_a_present_the_display_driver_takes() {
        let windows = crate::window_scene::ORDER_MAX as u32;
        let mut cb = nexus_gfx::CommandBuffer::new();
        let mut enc = cb
            .try_begin_render_pass(nexus_gfx::RenderPassDesc {
                color_attachments: alloc::vec![],
                width: 1920,
                height: 1080,
            })
            .expect("a pass");
        for i in 0..64u32 {
            let rect = nexus_gfx::TileRect { x: i, y: i, width: 64, height: 64 };
            enc.try_damage(rect, 0).expect("damage");
        }
        for i in 0..FRAME_LAYERS as u32 + windows {
            enc.try_layer_transform(i + 1, (3, -4), 200, 9_000, (960, 540)).expect("transform");
        }
        // Every layer the longest a layer is: a glass with its recipe and its shadow.
        for i in 0..FRAME_LAYERS as u32 {
            let mut glass = nexus_gfx::Layer::opaque(0, 0, 1240, 780, i, i);
            glass.shadow = Some(nexus_gfx::LayerShadow { blur: 24, offset_y: 8, alpha: 90 });
            glass.backdrop = Some(nexus_gfx::LayerBackdrop {
                blur_radius: 40,
                saturation_percent: 120,
                restore_halo_pad: 32,
                retained_src_y_offset: 0,
                cache: nexus_gfx::BackdropCache::Write {
                    cache_x: 0,
                    cache_row_abs: 0,
                    display_row_offset: 0,
                },
            });
            (glass.layer_id, glass.key) = (i + 1, (i + 1) << 16);
            enc.composite_layer_full(&glass, (1920, 1080)).expect("a layer of the budget");
        }
        enc.end_encoding();
        let mut body = alloc::vec![0u8; nexus_display_proto::PRESENT_BODY_MAX];
        let n = cb.serialize_into(&mut body).expect("the frame is a present's body");
        // One IPC frame (`nexus_abi::IPC_PAYLOAD_MAX`, an OS-only dependency here).
        let part = 8 * 1024 - nexus_display_proto::PRESENT_PART_HEADER_LEN;
        assert!(n > 3 * part, "{n}: the budget is past one frame");
        assert!(2 * n.div_ceil(part) <= 32 / 2, "{}", n.div_ceil(part));
        // And it is a buffer the driver's validation takes.
        let (back, used) = nexus_gfx::CommittedBuffer::deserialize_from(&body[..n]).expect("valid");
        assert_eq!(used, n);
        assert!(back.commands().len() >= 4 * FRAME_LAYERS);
    }

    /// The frame's budget holds every window an order lists with its body and three layers of
    /// its own beside the desktop's — sixty-four windows are not cut to their bodies.
    #[test]
    fn the_frames_budget_gives_every_window_of_a_full_order_layers_of_its_own() {
        let windows = crate::window_scene::ORDER_MAX;
        let bodies = windows + 2;
        let mut demand = alloc::vec![6usize; windows + 1];
        demand[windows] = 12;
        let mut share = alloc::vec![0usize; windows + 1];
        fair_shares(FRAME_LAYERS - bodies, &demand, &mut share);
        assert!(share.iter().all(|s| *s >= 2), "{share:?}");
        assert!(share.iter().sum::<usize>() + bodies <= FRAME_LAYERS);
    }

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

    #[test]
    fn test_reject_a_partial_set_of_pieces() {
        // A share short of the demand (the pieces counted) is not whole: the layer is drawn
        // whole, never as pieces around a region the share may then drop.
        assert!(!LayerQuota::new(5).covering(6).whole());
        assert!(LayerQuota::new(6).covering(6).whole());
        assert!(!LayerQuota::new(6).whole(), "a share never told its demand is not whole");
    }

    /// RFC-0121: the desktop body's pieces are taken all or none — a share short of the whole
    /// demand takes none of them and drops nothing (the one full-screen piece is drawn in the
    /// body's own slot), so its declared layers keep the share.
    #[test]
    fn test_reject_a_body_drawn_from_part_of_its_pieces() {
        let mut short = LayerQuota::new(9).covering(10);
        assert!(!short.take_all(3), "a share short of the demand");
        assert_eq!((short.dropped(), (0..9).filter(|_| short.take()).count()), (0, 9));
        let mut whole = LayerQuota::new(10).covering(10);
        assert!(whole.take_all(3), "the pieces past the body slot");
        assert_eq!((0..10).filter(|_| whole.take()).count(), 7, "the rest for the declared");
        let mut tight = LayerQuota::new(2).covering(2);
        assert!(!tight.take_all(3), "more than the share holds");
    }
}
