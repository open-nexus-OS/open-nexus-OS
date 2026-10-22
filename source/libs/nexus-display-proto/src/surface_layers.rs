// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0109 surface layers — what a client surface is made of and the two frames that
//! declare it: the STRUCTURE (`OP_SURFACE_TREE`: material regions, scroll layers with their
//! virtual bands, wash patches — applied with the surface's next present, atomic with the pixels
//! it names) and the MOTION (`OP_SURFACE_MOTION`: scroll offsets and wash layers — applied at
//! once, the newest superseding). Plus the one geometry check every receiver runs and the ring
//! mapping both ends share (the app writes content where the compositor reads it).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable (append-only)
//! TEST_COVERAGE: `surface_layers_tests.rs` (round trips, `test_reject_*` for every bound)
//!
//! A surface is `width × rows` pixels: rows `[0, height)` are the BASE layer, rows
//! `[height, rows)` the client's STORE, where its bands and patches live.

use crate::client_surface::{has_op, header, LayerDesc, HEADER_LEN, MAX_SURFACE_LAYERS};

/// client → windowd: the surface's structure, applied with its next present.
pub const OP_SURFACE_TREE: u8 = 33;
/// client → windowd: the surface's motion (scroll offsets, wash layers), applied at once.
pub const OP_SURFACE_MOTION: u8 = 34;

/// Scroll layers a surface may declare.
pub const MAX_SCROLL_LAYERS: usize = 4;
/// Wash patches a surface may declare.
pub const MAX_PATCHES: usize = 4;
/// Wash layers a motion may carry.
pub const MAX_WASHES: usize = 4;
/// A surface's rows (base + store), the windowd surface cap.
pub const MAX_SURFACE_ROWS: u16 = 3072;
/// A patch's side: one texel of margin around the 14×14 interior a wash samples.
pub const PATCH_SIDE: u16 = 16;

/// The scroll layer's axis.
pub const AXIS_VERTICAL: u8 = 0;
pub const AXIS_HORIZONTAL: u8 = 1;
/// Every pixel of the band is opaque: its slices occlude what lies below.
pub const BAND_OPAQUE: u8 = 1;

/// One scroll layer: the viewport (base coordinates), its axis and its virtual band — a ring of
/// `strip_len × strips` content positions along the axis, stored as `strips` strips placed next
/// to each other across the axis from (`bx`, `by`), and the content range it holds now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScrollLayerDesc {
    pub vx: u16,
    pub vy: u16,
    pub vw: u16,
    pub vh: u16,
    /// `AXIS_*`.
    pub axis: u8,
    /// `BAND_OPAQUE` or 0.
    pub flags: u8,
    pub bx: u16,
    pub by: u16,
    /// Content positions per strip (rows of a vertical strip, columns of a horizontal one).
    pub strip_len: u16,
    pub strips: u8,
    /// The content range the ring holds: `[valid_start, valid_start + valid_len)`; `0` = covered.
    pub valid_start: u32,
    pub valid_len: u16,
}

impl ScrollLayerDesc {
    /// The ring's capacity in content positions.
    #[must_use]
    pub fn capacity(&self) -> u32 {
        u32::from(self.strip_len) * u32::from(self.strips)
    }

    /// The viewport's length along the axis.
    #[must_use]
    pub fn viewport_len(&self) -> u32 {
        if self.axis == AXIS_VERTICAL {
            u32::from(self.vh)
        } else {
            u32::from(self.vw)
        }
    }

    /// The band's rectangle in the surface `(x, y, w, h)`.
    #[must_use]
    pub fn band_rect(&self) -> (u32, u32, u32, u32) {
        let (bx, by, len, k) = (
            u32::from(self.bx),
            u32::from(self.by),
            u32::from(self.strip_len),
            u32::from(self.strips),
        );
        if self.axis == AXIS_VERTICAL {
            (bx, by, k * u32::from(self.vw), len)
        } else {
            (bx, by, len, k * u32::from(self.vh))
        }
    }

    /// Where content position `p` lives in the surface: the pixel of its row (vertical) or
    /// column (horizontal) at the viewport's cross origin, and how many positions follow it in
    /// the same strip (the run a contiguous read or write may take). `None` for an empty ring.
    #[must_use]
    pub fn ring_place(&self, p: u32) -> Option<(u32, u32, u32)> {
        let cap = self.capacity();
        if cap == 0 {
            return None;
        }
        let r = p % cap;
        let len = u32::from(self.strip_len);
        let (strip, along) = (r / len, r % len);
        let run = len - along;
        Some(if self.axis == AXIS_VERTICAL {
            (u32::from(self.bx) + strip * u32::from(self.vw), u32::from(self.by) + along, run)
        } else {
            (u32::from(self.bx) + along, u32::from(self.by) + strip * u32::from(self.vh), run)
        })
    }

    /// Whether the band says every pixel of it is opaque.
    #[must_use]
    pub fn opaque(&self) -> bool {
        self.flags & BAND_OPAQUE != 0
    }
}

/// A wash patch: a 16×16 square of the store whose 14×14 interior is one colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Patch {
    pub x: u16,
    pub y: u16,
}

/// A surface's structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceTree {
    pub regions: [LayerDesc; MAX_SURFACE_LAYERS],
    pub region_count: usize,
    pub scrolls: [ScrollLayerDesc; MAX_SCROLL_LAYERS],
    pub scroll_count: usize,
    pub patches: [Patch; MAX_PATCHES],
    pub patch_count: usize,
}

impl SurfaceTree {
    /// The empty structure: no regions, no scroll layers, no patches.
    pub const EMPTY: Self = Self {
        regions: [LayerDesc {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            material: 0,
            glass_level: 0,
            radius: 0,
            shadow_alpha: 0,
        }; MAX_SURFACE_LAYERS],
        region_count: 0,
        scrolls: [ScrollLayerDesc {
            vx: 0,
            vy: 0,
            vw: 0,
            vh: 0,
            axis: 0,
            flags: 0,
            bx: 0,
            by: 0,
            strip_len: 0,
            strips: 0,
            valid_start: 0,
            valid_len: 0,
        }; MAX_SCROLL_LAYERS],
        scroll_count: 0,
        patches: [Patch { x: 0, y: 0 }; MAX_PATCHES],
        patch_count: 0,
    };

    pub fn regions(&self) -> &[LayerDesc] {
        &self.regions[..self.region_count.min(MAX_SURFACE_LAYERS)]
    }

    pub fn scrolls(&self) -> &[ScrollLayerDesc] {
        &self.scrolls[..self.scroll_count.min(MAX_SCROLL_LAYERS)]
    }

    pub fn patches(&self) -> &[Patch] {
        &self.patches[..self.patch_count.min(MAX_PATCHES)]
    }
}

impl Default for SurfaceTree {
    fn default() -> Self {
        Self::EMPTY
    }
}

const REGION_BYTES: usize = 12;
const SCROLL_BYTES: usize = 24;
const PATCH_BYTES: usize = 4;
const TREE_HEAD: usize = HEADER_LEN + 8;
/// The longest structure frame.
pub const SURFACE_TREE_MAX_LEN: usize = TREE_HEAD
    + MAX_SURFACE_LAYERS * REGION_BYTES
    + MAX_SCROLL_LAYERS * SCROLL_BYTES
    + MAX_PATCHES * PATCH_BYTES;

fn put16(out: &mut [u8], at: usize, v: u16) {
    out[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

fn put32(out: &mut [u8], at: usize, v: u32) {
    out[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn get16(f: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([f[at], f[at + 1]])
}

fn get32(f: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([f[at], f[at + 1], f[at + 2], f[at + 3]])
}

/// Encodes the structure (counts clamped to their bounds). Returns the frame length.
#[must_use]
pub fn encode_surface_tree(
    surface_id: u32,
    tree: &SurfaceTree,
    out: &mut [u8; SURFACE_TREE_MAX_LEN],
) -> usize {
    let (regions, scrolls, patches) = (tree.regions(), tree.scrolls(), tree.patches());
    out[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_TREE));
    put32(out, 4, surface_id);
    out[8] = regions.len() as u8;
    out[9] = scrolls.len() as u8;
    out[10] = patches.len() as u8;
    out[11] = 0;
    let mut pos = TREE_HEAD;
    for l in regions {
        put16(out, pos, l.x);
        put16(out, pos + 2, l.y);
        put16(out, pos + 4, l.w);
        put16(out, pos + 6, l.h);
        out[pos + 8] = l.material;
        out[pos + 9] = l.glass_level;
        out[pos + 10] = l.radius;
        out[pos + 11] = l.shadow_alpha;
        pos += REGION_BYTES;
    }
    for s in scrolls {
        put16(out, pos, s.vx);
        put16(out, pos + 2, s.vy);
        put16(out, pos + 4, s.vw);
        put16(out, pos + 6, s.vh);
        out[pos + 8] = s.axis;
        out[pos + 9] = s.flags;
        put16(out, pos + 10, s.bx);
        put16(out, pos + 12, s.by);
        put16(out, pos + 14, s.strip_len);
        out[pos + 16] = s.strips;
        out[pos + 17] = 0;
        put32(out, pos + 18, s.valid_start);
        put16(out, pos + 22, s.valid_len);
        pos += SCROLL_BYTES;
    }
    for p in patches {
        put16(out, pos, p.x);
        put16(out, pos + 2, p.y);
        pos += PATCH_BYTES;
    }
    pos
}

/// Decodes a structure frame into `out`: length-exact, counts within their bounds, reserved
/// bytes zero. Returns the surface id. The geometry is [`validate_tree`]'s.
#[must_use]
pub fn decode_surface_tree(frame: &[u8], out: &mut SurfaceTree) -> Option<u32> {
    if !has_op(frame, OP_SURFACE_TREE) || frame.len() < TREE_HEAD {
        return None;
    }
    let (regions, scrolls, patches) =
        (usize::from(frame[8]), usize::from(frame[9]), usize::from(frame[10]));
    if regions > MAX_SURFACE_LAYERS
        || scrolls > MAX_SCROLL_LAYERS
        || patches > MAX_PATCHES
        || frame[11] != 0
        || frame.len()
            != TREE_HEAD + regions * REGION_BYTES + scrolls * SCROLL_BYTES + patches * PATCH_BYTES
    {
        return None;
    }
    let mut tree = SurfaceTree::EMPTY;
    let mut pos = TREE_HEAD;
    for slot in tree.regions.iter_mut().take(regions) {
        *slot = LayerDesc {
            x: get16(frame, pos),
            y: get16(frame, pos + 2),
            w: get16(frame, pos + 4),
            h: get16(frame, pos + 6),
            material: frame[pos + 8],
            glass_level: frame[pos + 9],
            radius: frame[pos + 10],
            shadow_alpha: frame[pos + 11],
        };
        pos += REGION_BYTES;
    }
    for slot in tree.scrolls.iter_mut().take(scrolls) {
        if frame[pos + 17] != 0 {
            return None;
        }
        *slot = ScrollLayerDesc {
            vx: get16(frame, pos),
            vy: get16(frame, pos + 2),
            vw: get16(frame, pos + 4),
            vh: get16(frame, pos + 6),
            axis: frame[pos + 8],
            flags: frame[pos + 9],
            bx: get16(frame, pos + 10),
            by: get16(frame, pos + 12),
            strip_len: get16(frame, pos + 14),
            strips: frame[pos + 16],
            valid_start: get32(frame, pos + 18),
            valid_len: get16(frame, pos + 22),
        };
        pos += SCROLL_BYTES;
    }
    for slot in tree.patches.iter_mut().take(patches) {
        *slot = Patch { x: get16(frame, pos), y: get16(frame, pos + 2) };
        pos += PATCH_BYTES;
    }
    tree.region_count = regions;
    tree.scroll_count = scrolls;
    tree.patch_count = patches;
    *out = tree;
    Some(get32(frame, 4))
}

/// Why a structure was refused (RFC-0109: the frame is refused whole, the last valid one stays).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeError {
    /// A material region reaches outside the base layer.
    RegionOutside,
    /// A scroll layer's viewport is empty or reaches outside the base layer.
    ViewportOutside,
    /// An axis or flag the contract does not define.
    UnknownAxisOrFlag,
    /// An empty ring, or strips shorter than half the viewport (a visible window must fit in
    /// three pieces).
    StripTooShort,
    /// The valid range is longer than the ring.
    ValidPastCapacity,
    /// A band reaches outside the store's rows or width.
    BandOutside,
    /// A band lies over the base layer's rows.
    BandOverBase,
    /// A patch reaches outside the store.
    PatchOutside,
    /// Two bands, two patches or a band and a patch share pixels.
    Overlap,
}

type Rect = (u32, u32, u32, u32);

fn inside(r: Rect, w: u32, h: u32) -> bool {
    r.2 > 0 && r.3 > 0 && r.0.saturating_add(r.2) <= w && r.1.saturating_add(r.3) <= h
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.0 < b.0 + b.2 && b.0 < a.0 + a.2 && a.1 < b.1 + b.3 && b.1 < a.1 + a.3
}

/// The geometry check of a structure against its surface (`width × rows`, the first `height`
/// rows the base, columns `[0, store_w)` of the rows below it the store): every region and
/// viewport inside the base, every band and patch in the store and none sharing a pixel with
/// another. A structure that passes makes windowd sample only this surface's own atlas regions.
pub fn validate_tree(
    tree: &SurfaceTree,
    width: u16,
    height: u16,
    rows: u16,
    store_w: u16,
) -> Result<(), TreeError> {
    let (w, h, rows) = (u32::from(width), u32::from(height), u32::from(rows));
    let store_w = u32::from(store_w).min(w);
    for r in tree.regions() {
        let rect = (u32::from(r.x), u32::from(r.y), u32::from(r.w), u32::from(r.h));
        if !inside(rect, w, h) {
            return Err(TreeError::RegionOutside);
        }
    }
    let mut store: [Rect; MAX_SCROLL_LAYERS + MAX_PATCHES] =
        [(0, 0, 0, 0); MAX_SCROLL_LAYERS + MAX_PATCHES];
    let mut n = 0;
    for s in tree.scrolls() {
        let viewport = (u32::from(s.vx), u32::from(s.vy), u32::from(s.vw), u32::from(s.vh));
        if !inside(viewport, w, h) {
            return Err(TreeError::ViewportOutside);
        }
        if s.axis > AXIS_HORIZONTAL || s.flags & !BAND_OPAQUE != 0 {
            return Err(TreeError::UnknownAxisOrFlag);
        }
        if s.strips == 0 || u32::from(s.strip_len) * 2 < s.viewport_len() {
            return Err(TreeError::StripTooShort);
        }
        if u32::from(s.valid_len) > s.capacity() {
            return Err(TreeError::ValidPastCapacity);
        }
        let band = s.band_rect();
        if !inside(band, store_w, rows) {
            return Err(TreeError::BandOutside);
        }
        if band.1 < h {
            return Err(TreeError::BandOverBase);
        }
        store[n] = band;
        n += 1;
    }
    for p in tree.patches() {
        let side = u32::from(PATCH_SIDE);
        let rect = (u32::from(p.x), u32::from(p.y), side, side);
        if !inside(rect, store_w, rows) || rect.1 < h {
            return Err(TreeError::PatchOutside);
        }
        store[n] = rect;
        n += 1;
    }
    for i in 0..n {
        if store[i + 1..n].iter().any(|b| overlaps(store[i], *b)) {
            return Err(TreeError::Overlap);
        }
    }
    Ok(())
}

/// One wash layer: patch `patch` spread over the rectangle (`x`, `y`, `w`, `h`) in its parent's
/// content coordinates (the base: surface coordinates; scroll layer `k`: relative to its
/// viewport's origin at offset 0), with a corner radius and an opacity, over the parent's content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Wash {
    /// `0` = the base, `k` = scroll layer `k` (1-based).
    pub parent: u8,
    pub patch: u8,
    pub radius: u8,
    pub opacity: u8,
    pub x: i32,
    pub y: i32,
    pub w: u16,
    pub h: u16,
}

/// A surface's motion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SurfaceMotion {
    /// The scroll layers' offsets along their axes, in the structure's order.
    pub offsets: [i32; MAX_SCROLL_LAYERS],
    pub offset_count: usize,
    pub washes: [Wash; MAX_WASHES],
    pub wash_count: usize,
}

impl SurfaceMotion {
    pub fn offsets(&self) -> &[i32] {
        &self.offsets[..self.offset_count.min(MAX_SCROLL_LAYERS)]
    }

    pub fn washes(&self) -> &[Wash] {
        &self.washes[..self.wash_count.min(MAX_WASHES)]
    }
}

const MOTION_HEAD: usize = HEADER_LEN + 8;
const WASH_BYTES: usize = 16;
/// The longest motion frame.
pub const SURFACE_MOTION_MAX_LEN: usize =
    MOTION_HEAD + MAX_SCROLL_LAYERS * 4 + MAX_WASHES * WASH_BYTES;

/// Encodes the motion (counts clamped to their bounds). Returns the frame length.
#[must_use]
pub fn encode_surface_motion(
    surface_id: u32,
    motion: &SurfaceMotion,
    out: &mut [u8; SURFACE_MOTION_MAX_LEN],
) -> usize {
    let (offsets, washes) = (motion.offsets(), motion.washes());
    out[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_MOTION));
    put32(out, 4, surface_id);
    out[8] = offsets.len() as u8;
    out[9] = washes.len() as u8;
    put16(out, 10, 0);
    let mut pos = MOTION_HEAD;
    for o in offsets {
        put32(out, pos, *o as u32);
        pos += 4;
    }
    for w in washes {
        out[pos] = w.parent;
        out[pos + 1] = w.patch;
        out[pos + 2] = w.radius;
        out[pos + 3] = w.opacity;
        put32(out, pos + 4, w.x as u32);
        put32(out, pos + 8, w.y as u32);
        put16(out, pos + 12, w.w);
        put16(out, pos + 14, w.h);
        pos += WASH_BYTES;
    }
    pos
}

/// Decodes a motion frame: length-exact, counts within their bounds, reserved bytes zero.
/// Returns the surface id. Whether it names existing layers and patches is
/// [`validate_motion`]'s.
#[must_use]
pub fn decode_surface_motion(frame: &[u8], out: &mut SurfaceMotion) -> Option<u32> {
    if !has_op(frame, OP_SURFACE_MOTION) || frame.len() < MOTION_HEAD {
        return None;
    }
    let (offsets, washes) = (usize::from(frame[8]), usize::from(frame[9]));
    if offsets > MAX_SCROLL_LAYERS
        || washes > MAX_WASHES
        || get16(frame, 10) != 0
        || frame.len() != MOTION_HEAD + offsets * 4 + washes * WASH_BYTES
    {
        return None;
    }
    let mut motion = SurfaceMotion::default();
    let mut pos = MOTION_HEAD;
    for slot in motion.offsets.iter_mut().take(offsets) {
        *slot = get32(frame, pos) as i32;
        pos += 4;
    }
    for slot in motion.washes.iter_mut().take(washes) {
        *slot = Wash {
            parent: frame[pos],
            patch: frame[pos + 1],
            radius: frame[pos + 2],
            opacity: frame[pos + 3],
            x: get32(frame, pos + 4) as i32,
            y: get32(frame, pos + 8) as i32,
            w: get16(frame, pos + 12),
            h: get16(frame, pos + 14),
        };
        pos += WASH_BYTES;
    }
    motion.offset_count = offsets;
    motion.wash_count = washes;
    *out = motion;
    Some(get32(frame, 4))
}

/// Why a motion was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionError {
    /// A wash names a patch the structure does not declare.
    MissingPatch,
    /// A wash names a parent the structure does not declare.
    MissingParent,
    /// A wash with no area.
    EmptyWash,
}

/// Whether a motion names only what `tree` declares. Offsets beyond the structure's scroll
/// layers are ignored by the receiver (a motion may race a structure change).
pub fn validate_motion(motion: &SurfaceMotion, tree: &SurfaceTree) -> Result<(), MotionError> {
    for w in motion.washes() {
        if usize::from(w.patch) >= tree.patches().len() {
            return Err(MotionError::MissingPatch);
        }
        if usize::from(w.parent) > tree.scrolls().len() {
            return Err(MotionError::MissingParent);
        }
        if w.w == 0 || w.h == 0 {
            return Err(MotionError::EmptyWash);
        }
    }
    Ok(())
}
