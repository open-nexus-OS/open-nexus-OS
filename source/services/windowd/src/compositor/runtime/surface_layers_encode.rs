// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: windowd compositor runtime — the ONE encoder of a client surface's declared layers
//! (RFC-0109), for an app window and for the desktop alike: its motion layers' own pixels (drawn
//! where the base would hold them, from the store — TASK-0354 P2), its material regions (the R1
//! seam; a region a motion layer claims samples the layer's pixels), each uncovered scroll
//! layer's slices (the band's visible window in its pieces, at the offset the client's motion
//! declared) and its wash layers (a patch spread over a rectangle, at its declared opacity).
//! Whatever belongs to a moving motion layer carries its transform id (`motion_geometry::
//! layer_id`) — the present's `Command::LayerTransform` moves and fades it on the GPU; a layer's
//! backdrop (a modal's scrim: one patch spread over the surface) fades with it below its pixels.
//! A layer declared in front and what is its own come last: in a window after its body's
//! layers, on the desktop in a pass of their own above the windows, the bar and the panels
//! (`Pass::DesktopFront` — the shell's modal and toast). Every layer carries a key unique in the
//! frame — the planner's match from one frame to the next (RFC-0101 §8 amended).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the geometry in `surface_layers.rs`; the encode on the QEMU lanes.

use crate::layer_geometry::{pieces, wash_place, Piece, SurfaceLayers, MAX_PIECES};
use crate::layer_quota::LayerQuota;
use crate::motion_geometry::{backdrop_id, layer_id as motion_id, region_owner, scroll_owner};
use crate::surface_regions::SurfaceRegions;
use nexus_display_proto::client_surface::{
    LayerDesc, GLASS_DOCK, GLASS_PANEL, GLASS_THIN, GLASS_ULTRA_THIN, MATERIAL_GLASS,
    MATERIAL_OPAQUE,
};
use nexus_display_proto::surface_layers::{MAX_SCROLL_LAYERS, PATCH_SIDE};
use nexus_gfx::command::layer_wire;
use nexus_gfx::{Layer, RenderCommandEncoder};

/// The key's owner of the desktop surface's layers — the desktop's key (RFC-0114 §7: windows
/// are their own keys, `1..=0xFFFE`).
pub(crate) const DESKTOP_OWNER: u32 = super::slots::DESKTOP_KEY as u32;
/// Layer kinds in a key (`owner << 16 | kind << 8 | index`, `nexus_gfx::command::layer_wire`:
/// gpud reads a window's body off the key after a restart, RFC-0112 Phase 3).
pub(crate) const KIND_BODY: u32 = layer_wire::LAYER_KIND_BODY;
pub(crate) const KIND_REGION: u32 = layer_wire::LAYER_KIND_REGION;
pub(crate) const KIND_SLICE: u32 = layer_wire::LAYER_KIND_SLICE;
pub(crate) const KIND_WASH: u32 = layer_wire::LAYER_KIND_WASH;
pub(crate) const KIND_SOLID: u32 = layer_wire::LAYER_KIND_SOLID;
pub(crate) const KIND_BAR: u32 = layer_wire::LAYER_KIND_BAR;
/// RFC-0113 §5: the caret over the surface's field.
pub(crate) const KIND_CARET: u32 = layer_wire::LAYER_KIND_CARET;

/// A layer's key: the window's key, what of the window it is, which one of those.
pub(crate) const fn key(owner: u32, kind: u32, index: u32) -> u32 {
    layer_wire::layer_key(owner, kind, index)
}

/// The backdrop blur of a region's glass level (RFC-0116: the level is the bucket of a backdrop —
/// `regular` for a panel or a window, `thin`, `ultraThin`, `thick` for the dock), read from the
/// theme's backdrops so the compositor and the token set cannot disagree on a radius.
pub(crate) fn region_blur(l: &LayerDesc) -> u32 {
    use nexus_theme_tokens::{BackdropToken, DarkTokens, Tokens};
    if l.material == MATERIAL_OPAQUE {
        return 0;
    }
    let backdrop = match l.glass_level {
        GLASS_THIN => BackdropToken::Thin,
        GLASS_ULTRA_THIN => BackdropToken::UltraThin,
        GLASS_DOCK => BackdropToken::Thick,
        // GLASS_PANEL, GLASS_WINDOW — and an unknown level takes the regular backdrop.
        _ => BackdropToken::Regular,
    };
    // The backdrops are theme-invariant (authored once in the base theme).
    u32::from(DarkTokens.backdrop(backdrop).blur)
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
    /// The desktop in front of everything: the layers it declared in front (its modal, its
    /// toast) with their backdrops, regions, slices and washes — above the windows, the bar and
    /// the panels.
    DesktopFront,
}

/// A surface's layers as this frame composes them — a copy taken before the encoder borrows the
/// runtime.
#[derive(Clone, Copy)]
pub(crate) struct LayerSnap {
    pub(crate) layers: SurfaceLayers,
    /// Where the surface's base and store are in its buffer's newest slot (RFC-0114).
    pub(crate) regions: SurfaceRegions,
    /// Where the surface's origin lands on the screen.
    pub(crate) origin: (u32, u32),
    /// The base layer's size.
    pub(crate) base: (u32, u32),
    /// The key's owner (the window's key, or [`DESKTOP_OWNER`]).
    pub(crate) owner: u32,
    /// The window transform's id (the window's key; `0` = the desktop).
    pub(crate) layer_id: u32,
    /// The motion layers moving this frame (bit `k`: layer `k`, `surface_motion::live`).
    pub(crate) live: u8,
}

type Rect = (u32, u32, u32, u32);

fn inside(inner: Rect, outer: Rect) -> bool {
    inner.0 >= outer.0
        && inner.1 >= outer.1
        && inner.0 + inner.2 <= outer.0 + outer.2
        && inner.1 + inner.3 <= outer.1 + outer.3
}

/// The first store key index of the motion layers' own pixels (slices use `k * MAX_PIECES + j`).
const MOTION_KEY_BASE: u32 = 0x80;
/// The first wash key index of the motion layers' backdrops (washes use their index).
const BACKDROP_KEY_BASE: u32 = 0x80;

impl LayerSnap {
    /// The transform id of what belongs to motion layer `motion` (`None`: to no motion layer):
    /// the motion's while it moves, the surface's own at rest.
    fn id_of(&self, motion: Option<usize>) -> u32 {
        match motion {
            Some(k) if self.live & (1 << k) != 0 => motion_id(self.owner, k),
            _ => self.layer_id,
        }
    }

    /// Whether what belongs to motion layer `motion` is drawn in front (`None`: to no layer — the
    /// base's, never in front).
    fn in_front(&self, motion: Option<usize>) -> bool {
        motion.and_then(|k| self.layers.tree.motions().get(k)).is_some_and(|m| m.front)
    }

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
            Pass::Window | Pass::DesktopFront => true,
            Pass::DesktopBelow => !self.in_panel(r),
            Pass::DesktopAbove => self.in_panel(r),
        }
    }

    /// Encodes this pass's part of the surface's layers within the surface's share of the frame
    /// (`quota`, RFC-0109): past it the surface's last layers are not composed, and counted. A
    /// window's layers declared in front follow all its others; the desktop's are their own pass.
    pub(crate) fn encode(
        &self,
        enc: &mut RenderCommandEncoder<'_>,
        pass: Pass,
        mode: (u32, u32),
        quota: &mut LayerQuota,
    ) {
        match pass {
            Pass::Window => {
                self.encode_part(enc, pass, false, mode, quota);
                self.encode_part(enc, pass, true, mode, quota);
            }
            Pass::DesktopFront => self.encode_part(enc, pass, true, mode, quota),
            Pass::DesktopBelow | Pass::DesktopAbove => {
                self.encode_part(enc, pass, false, mode, quota);
            }
        }
    }

    /// The part of this pass's layers that is (`front`) or is not in front.
    fn encode_part(
        &self,
        enc: &mut RenderCommandEncoder<'_>,
        pass: Pass,
        front: bool,
        mode: (u32, u32),
        quota: &mut LayerQuota,
    ) {
        self.encode_motions(enc, pass, front, mode, quota);
        self.encode_regions(enc, pass, front, mode, quota);
        self.encode_slices(enc, pass, front, mode, quota);
        self.encode_washes(enc, pass, front, mode, quota);
    }

    /// Each motion layer's own pixels (TASK-0354 P2), from the store, where the base would hold
    /// them — over its backdrop, when it has one: drawn with the base — first in a window, in the
    /// desktop's pass below the windows — or, in front, after everything.
    fn encode_motions(
        &self,
        enc: &mut RenderCommandEncoder<'_>,
        pass: Pass,
        front: bool,
        mode: (u32, u32),
        quota: &mut LayerQuota,
    ) {
        if pass == Pass::DesktopAbove {
            return;
        }
        for (k, m) in self.layers.tree.motions().iter().enumerate() {
            if m.front != front {
                continue;
            }
            if let Some((patch, opacity)) = m.backdrop() {
                self.encode_backdrop(enc, k, (patch, opacity), mode, quota);
            }
            let p = Piece {
                src: (u32::from(m.sx), u32::from(m.sy)),
                dst: (u32::from(m.x), u32::from(m.y)),
                size: (u32::from(m.w), u32::from(m.h)),
            };
            let draw = Draw {
                opacity: 255,
                radius: 0,
                opaque: false,
                spread: None,
                band: (0, 0),
                key: key(self.owner, KIND_SLICE, MOTION_KEY_BASE + k as u32),
                layer_id: self.id_of(Some(k)),
            };
            self.composite(enc, &p, draw, mode, quota);
        }
    }

    /// Motion layer `k`'s backdrop: patch `patch` spread over the whole surface at `opacity` —
    /// fading with the layer (its own transform id: the layer's opacity, not its offset).
    fn encode_backdrop(
        &self,
        enc: &mut RenderCommandEncoder<'_>,
        k: usize,
        (patch, opacity): (usize, u8),
        mode: (u32, u32),
        quota: &mut LayerQuota,
    ) {
        let Some(at) = self.layers.tree.patches().get(patch) else { return };
        let p =
            Piece { src: (u32::from(at.x) + 1, u32::from(at.y) + 1), dst: (0, 0), size: self.base };
        let interior = u32::from(PATCH_SIDE) - 2;
        let layer_id =
            if self.live & (1 << k) != 0 { backdrop_id(self.owner, k) } else { self.layer_id };
        let draw = Draw {
            opacity: u32::from(opacity),
            radius: 0,
            opaque: false,
            spread: Some((interior, interior)),
            band: (0, 0),
            key: key(self.owner, KIND_WASH, BACKDROP_KEY_BASE + k as u32),
            layer_id,
        };
        self.composite(enc, &p, draw, mode, quota);
    }

    fn encode_regions(
        &self,
        enc: &mut RenderCommandEncoder<'_>,
        pass: Pass,
        front: bool,
        mode: (u32, u32),
        quota: &mut LayerQuota,
    ) {
        let tree = &self.layers.tree;
        for (i, l) in tree.regions().iter().enumerate() {
            // A region a motion layer claims is the layer's pixels, in the store (TASK-0354 P2) —
            // and in front with the layer.
            let owner = region_owner(tree, i);
            if self.in_front(owner) != front {
                continue;
            }
            let glass = l.material == MATERIAL_GLASS;
            let draw = match pass {
                Pass::Window => glass || l.material == MATERIAL_OPAQUE,
                // The desktop base is opaque already: only its glass regions are layers.
                Pass::DesktopBelow => glass && l.glass_level != GLASS_PANEL,
                Pass::DesktopAbove => glass && l.glass_level == GLASS_PANEL,
                Pass::DesktopFront => glass,
            };
            let src = match owner.and_then(|k| tree.motions().get(k)) {
                Some(m) => {
                    let (x, y) = m.store_of(u32::from(l.x), u32::from(l.y));
                    self.regions.store_place(x, y)
                }
                None => self
                    .regions
                    .place(u32::from(l.y))
                    .map(|(row, col, _)| (row, col + u32::from(l.x))),
            };
            let Some((row, src_x)) = src else { continue };
            if !draw || !quota.take() {
                continue;
            }
            crate::compositor::shell_window::composite_material_glass(
                enc,
                crate::compositor::shell_window::MaterialLayerParams {
                    src_row_abs: row,
                    src_x,
                    width: u32::from(l.w),
                    height: u32::from(l.h),
                    dst_x: self.origin.0 + u32::from(l.x),
                    dst_y: self.origin.1 + u32::from(l.y),
                    corner_radius: u32::from(l.radius),
                    shadow_alpha: u32::from(l.shadow_alpha),
                    blur_radius: region_blur(l),
                    layer_id: self.id_of(owner),
                    opaque: l.material == MATERIAL_OPAQUE,
                    key: key(self.owner, KIND_REGION, i as u32),
                    source: (self.regions.source, self.regions.slot, self.regions.generation),
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
        front: bool,
        mode: (u32, u32),
        quota: &mut LayerQuota,
    ) {
        for (k, s) in self.layers.tree.scrolls().iter().enumerate().take(MAX_SCROLL_LAYERS) {
            let viewport = (u32::from(s.vx), u32::from(s.vy), u32::from(s.vw), u32::from(s.vh));
            let motion = scroll_owner(&self.layers.tree, k);
            if self.in_front(motion) != front || !self.drawn_in(pass, viewport) {
                continue;
            }
            let band = s.band_rect();
            // A band samples the store's rows (RFC-0109): no store, nothing to compose.
            let Some((band_row, _)) = self.regions.store_place(band.0, band.1) else { continue };
            let mut out = [Piece::default(); MAX_PIECES];
            let n = pieces(s, self.layers.offset(k), &mut out);
            let layer_id = self.id_of(motion);
            for (j, p) in out[..n].iter().enumerate() {
                let draw = Draw {
                    opacity: 255,
                    radius: 0,
                    opaque: s.opaque(),
                    spread: None,
                    band: (band_row, band.3),
                    key: key(self.owner, KIND_SLICE, (k * MAX_PIECES + j) as u32),
                    layer_id,
                };
                self.composite(enc, p, draw, mode, quota);
            }
        }
    }

    fn encode_washes(
        &self,
        enc: &mut RenderCommandEncoder<'_>,
        pass: Pass,
        front: bool,
        mode: (u32, u32),
        quota: &mut LayerQuota,
    ) {
        for (i, w) in self.layers.motion.washes().iter().enumerate() {
            let Some(p) = wash_place(w, &self.layers, self.base) else { continue };
            // A wash belongs to the pass of its parent's viewport (the base or a motion layer: its
            // own rectangle) and moves with what its parent moves with.
            let (home, motion) = match (w.motion_parent(), usize::from(w.parent)) {
                (Some(m), _) => ((p.dst.0, p.dst.1, p.size.0, p.size.1), Some(m)),
                (None, 0) => ((p.dst.0, p.dst.1, p.size.0, p.size.1), None),
                (None, k) => match self.layers.tree.scrolls().get(k - 1) {
                    Some(s) => (
                        (u32::from(s.vx), u32::from(s.vy), u32::from(s.vw), u32::from(s.vh)),
                        scroll_owner(&self.layers.tree, k - 1),
                    ),
                    None => continue,
                },
            };
            if self.in_front(motion) != front || !self.drawn_in(pass, home) {
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
                layer_id: self.id_of(motion),
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
        // Slices and washes sample the store's rows of the slot (RFC-0109).
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
                layer_id: draw.layer_id,
                content_epoch: self.regions.generation,
                scroll_band_top_abs: draw.band.0,
                scroll_band_h: draw.band.1,
                source: self.regions.source,
                slot: self.regions.slot,
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
/// and the height), its key, and the transform id it carries.
#[derive(Clone, Copy)]
struct Draw {
    opacity: u32,
    radius: u32,
    opaque: bool,
    spread: Option<(u32, u32)>,
    band: (u32, u32),
    key: u32,
    layer_id: u32,
}
