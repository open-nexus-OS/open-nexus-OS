// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: windowd compositor runtime — the ONE encoder of a client surface's declared layers
//! (RFC-0109), for an app window and for the desktop alike: its material regions (the R1 seam),
//! each uncovered scroll layer's slices (the band's visible window in its pieces, at the offset
//! the client's motion declared) and its wash layers (a patch spread over a rectangle, at its
//! declared opacity). Every layer carries a key unique in the frame — the planner's match from
//! one frame to the next (RFC-0101 §8 amended).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the geometry in `surface_layers.rs`; the encode on the QEMU lanes.

use crate::layer_geometry::{pieces, wash_place, LayerQuota, Piece, SurfaceLayers, MAX_PIECES};
use crate::surface_regions::SurfaceRegions;
use nexus_display_proto::client_surface::{
    LayerDesc, GLASS_CARD, GLASS_OVERLAY, GLASS_PANEL, GLASS_SUBTLE, MATERIAL_GLASS,
    MATERIAL_OPAQUE,
};
use nexus_display_proto::surface_layers::{MAX_SCROLL_LAYERS, PATCH_SIDE};
use nexus_gfx::{Layer, RenderCommandEncoder};

/// The key's owner of the desktop surface's layers (windows are `slot + 1`).
pub(crate) const DESKTOP_OWNER: u32 = 0x10;
/// Layer kinds in a key (`owner << 16 | kind << 8 | index`).
pub(crate) const KIND_BODY: u32 = 1;
pub(crate) const KIND_REGION: u32 = 2;
pub(crate) const KIND_SLICE: u32 = 3;
pub(crate) const KIND_WASH: u32 = 4;
pub(crate) const KIND_SOLID: u32 = 5;
pub(crate) const KIND_BAR: u32 = 6;

/// A layer's key: who drew it, what it is, which one.
pub(crate) const fn key(owner: u32, kind: u32, index: u32) -> u32 {
    (owner << 16) | (kind << 8) | (index & 0xff)
}

/// The backdrop blur of a region's glass level (the level is a blur bucket — materials.md).
pub(crate) fn region_blur(l: &LayerDesc) -> u32 {
    match l.glass_level {
        _ if l.material == MATERIAL_OPAQUE => 0,
        GLASS_PANEL | GLASS_OVERLAY => 40,
        GLASS_CARD => 20,
        GLASS_SUBTLE => 8,
        _ => 30,
    }
}

/// Which part of a surface's layers a scene pass draws.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pass {
    /// An app window: every region, slice and wash.
    Window,
    /// The desktop below the windows: its glass regions but the panels, and what lies outside a
    /// panel.
    DesktopBelow,
    /// The desktop above the windows (the shell's panels — the Control Center, the launcher):
    /// its panel regions and what lies inside one.
    DesktopAbove,
}

/// A surface's layers as this frame composes them — a copy taken before the encoder borrows the
/// runtime.
#[derive(Clone, Copy)]
pub(crate) struct LayerSnap {
    pub(crate) layers: SurfaceLayers,
    /// Where the surface's base and store live in the atlas.
    pub(crate) regions: SurfaceRegions,
    /// Where the surface's origin lands on the screen.
    pub(crate) origin: (u32, u32),
    /// The base layer's size.
    pub(crate) base: (u32, u32),
    /// The key's owner (`slot + 1`, or [`DESKTOP_OWNER`]).
    pub(crate) owner: u32,
    /// The window transform's id (`slot + 1`; `0` = the desktop).
    pub(crate) layer_id: u32,
}

type Rect = (u32, u32, u32, u32);

fn inside(inner: Rect, outer: Rect) -> bool {
    inner.0 >= outer.0
        && inner.1 >= outer.1
        && inner.0 + inner.2 <= outer.0 + outer.2
        && inner.1 + inner.3 <= outer.1 + outer.3
}

impl LayerSnap {
    /// Whether a rectangle of the surface lies inside one of its panel-level glass regions.
    fn in_panel(&self, r: Rect) -> bool {
        self.layers.tree.regions().iter().any(|l| {
            l.material == MATERIAL_GLASS
                && l.glass_level == GLASS_PANEL
                && inside(r, (u32::from(l.x), u32::from(l.y), u32::from(l.w), u32::from(l.h)))
        })
    }

    /// Whether this pass draws something at `r` of the surface.
    fn drawn_in(&self, pass: Pass, r: Rect) -> bool {
        match pass {
            Pass::Window => true,
            Pass::DesktopBelow => !self.in_panel(r),
            Pass::DesktopAbove => self.in_panel(r),
        }
    }

    /// Encodes this pass's part of the surface's layers within the surface's share of the frame
    /// (`quota`, RFC-0109): past it the surface's last layers are not composed, and counted.
    pub(crate) fn encode(
        &self,
        enc: &mut RenderCommandEncoder<'_>,
        pass: Pass,
        mode: (u32, u32),
        quota: &mut LayerQuota,
    ) {
        self.encode_regions(enc, pass, mode, quota);
        self.encode_slices(enc, pass, mode, quota);
        self.encode_washes(enc, pass, mode, quota);
    }

    fn encode_regions(
        &self,
        enc: &mut RenderCommandEncoder<'_>,
        pass: Pass,
        mode: (u32, u32),
        quota: &mut LayerQuota,
    ) {
        for (i, l) in self.layers.tree.regions().iter().enumerate() {
            let glass = l.material == MATERIAL_GLASS;
            let draw = match pass {
                Pass::Window => glass || l.material == MATERIAL_OPAQUE,
                // The desktop base is opaque already: only its glass regions are layers.
                Pass::DesktopBelow => glass && l.glass_level != GLASS_PANEL,
                Pass::DesktopAbove => glass && l.glass_level == GLASS_PANEL,
            };
            let Some((row, col, _)) = self.regions.place(u32::from(l.y)) else { continue };
            if !draw || !quota.take() {
                continue;
            }
            crate::compositor::shell_window::composite_material_glass(
                enc,
                crate::compositor::shell_window::MaterialLayerParams {
                    src_row_abs: row,
                    src_x: col + u32::from(l.x),
                    width: u32::from(l.w),
                    height: u32::from(l.h),
                    dst_x: self.origin.0 + u32::from(l.x),
                    dst_y: self.origin.1 + u32::from(l.y),
                    corner_radius: u32::from(l.radius),
                    shadow_alpha: u32::from(l.shadow_alpha),
                    blur_radius: region_blur(l),
                    layer_id: self.layer_id,
                    opaque: l.material == MATERIAL_OPAQUE,
                    key: key(self.owner, KIND_REGION, i as u32),
                },
                mode.0,
                mode.1,
            );
        }
    }

    fn encode_slices(
        &self,
        enc: &mut RenderCommandEncoder<'_>,
        pass: Pass,
        mode: (u32, u32),
        quota: &mut LayerQuota,
    ) {
        for (k, s) in self.layers.tree.scrolls().iter().enumerate().take(MAX_SCROLL_LAYERS) {
            let viewport = (u32::from(s.vx), u32::from(s.vy), u32::from(s.vw), u32::from(s.vh));
            if !self.drawn_in(pass, viewport) {
                continue;
            }
            let band = s.band_rect();
            // A band samples the store's region (RFC-0109): none placed, nothing to compose.
            let Some((band_row, _)) = self.regions.store_place(band.0, band.1) else { continue };
            let mut out = [Piece::default(); MAX_PIECES];
            let n = pieces(s, self.layers.offset(k), &mut out);
            for (j, p) in out[..n].iter().enumerate() {
                let draw = Draw {
                    opacity: 255,
                    radius: 0,
                    opaque: s.opaque(),
                    spread: None,
                    band: (band_row, band.3),
                    key: key(self.owner, KIND_SLICE, (k * MAX_PIECES + j) as u32),
                };
                self.composite(enc, p, draw, mode, quota);
            }
        }
    }

    fn encode_washes(
        &self,
        enc: &mut RenderCommandEncoder<'_>,
        pass: Pass,
        mode: (u32, u32),
        quota: &mut LayerQuota,
    ) {
        for (i, w) in self.layers.motion.washes().iter().enumerate() {
            let Some(p) = wash_place(w, &self.layers, self.base) else { continue };
            // A wash belongs to the pass of its parent's viewport (the base: its own rectangle).
            let home = match usize::from(w.parent) {
                0 => (p.dst.0, p.dst.1, p.size.0, p.size.1),
                k => match self.layers.tree.scrolls().get(k - 1) {
                    Some(s) => (u32::from(s.vx), u32::from(s.vy), u32::from(s.vw), u32::from(s.vh)),
                    None => continue,
                },
            };
            if !self.drawn_in(pass, home) {
                continue;
            }
            let interior = u32::from(PATCH_SIDE) - 2;
            let draw = Draw {
                opacity: u32::from(w.opacity),
                radius: u32::from(w.radius),
                opaque: false,
                spread: Some((interior, interior)),
                band: (0, 0),
                key: key(self.owner, KIND_WASH, i as u32),
            };
            self.composite(enc, &p, draw, mode, quota);
        }
    }

    /// One piece of the surface composited at its place on the screen, cut at the display's edge,
    /// when it fits the surface's share.
    fn composite(
        &self,
        enc: &mut RenderCommandEncoder<'_>,
        p: &Piece,
        draw: Draw,
        mode: (u32, u32),
        quota: &mut LayerQuota,
    ) {
        let (dst_x, dst_y) = (self.origin.0 + p.dst.0, self.origin.1 + p.dst.1);
        // Slices and washes sample the store's region (RFC-0109).
        let Some((src_row, src_col)) = self.regions.store_place(p.src.0, p.src.1) else { return };
        if dst_x >= mode.0 || dst_y >= mode.1 || p.size.0 == 0 || p.size.1 == 0 || !quota.take() {
            return;
        }
        let width = p.size.0.min(mode.0 - dst_x);
        let height = p.size.1.min(mode.1 - dst_y);
        let (content_w, content_h) = draw.spread.unwrap_or((0, 0));
        let _ = enc.composite_layer_full(
            &Layer {
                src_row_abs: src_row,
                src_x: src_col,
                width,
                height,
                content_w,
                content_h,
                dst_x,
                dst_y,
                opacity: draw.opacity,
                corner_radius: draw.radius,
                layer_id: self.layer_id,
                content_epoch: crate::atlas::atlas_content_epoch(),
                scroll_band_top_abs: draw.band.0,
                scroll_band_h: draw.band.1,
                shadow: None,
                backdrop: None,
                opaque: draw.opaque,
                solid: draw.spread.is_some(),
                key: draw.key,
            },
            mode,
        );
    }
}

/// How a piece is drawn and which layer it is: its opacity and corner radius, whether it is
/// opaque, for a patch the interior spread over it, for a slice its band (the absolute top row
/// and the height), and its key.
#[derive(Clone, Copy)]
struct Draw {
    opacity: u32,
    radius: u32,
    opaque: bool,
    spread: Option<(u32, u32)>,
    band: (u32, u32),
    key: u32,
}
