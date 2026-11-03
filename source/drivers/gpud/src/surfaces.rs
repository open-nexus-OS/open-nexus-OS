// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The app surfaces gpud made for their owners (RFC-0114 §1–§2, ADR-0092): one object
//! per surface, `slots` slots of the RFC-0109 layout at a 256-byte stride, made for the device
//! that samples it and charged to the app it is for (its kernel id — windowd's word, served to
//! windowd alone). The app draws into a slot, the GPU reads the same memory: windowd composes by
//! naming the object (`source`) and the slot. This table is backend-neutral: the ids, the owners,
//! the geometry every present's layers are checked against, the deferred free — an object windowd
//! frees is unbound and closed only once the last frame that could read it retired. How a
//! backend reads an object (a CPU mapping, a virgl resource, a binding in the GPU's MMU) is the
//! backend's. After a compositor restart (RFC-0112 Phase 3) the table keeps every object as an
//! ORPHAN with the place the last frame drew its body at: the new compositor adopts the placed
//! ones (its ghosts) and gpud releases the rest.
//! OWNERS: @gpu @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests below (ids, the size class, a layer's bounds, the deferred free, the
//!   ceiling) incl. `test_reject_*`
//! ADR: docs/adr/0092-a-surface-is-a-buffer-the-app-draws-and-the-gpu-reads-gpud-allocates-it-for-its-owner-windowd-composes-by-reference.md

use alloc::vec::Vec;

use nexus_display_proto::surface_alloc::{self as wire, AllocRequest};
use nexus_gfx::command::buffer::Command;
use nexus_gfx::command::layer_wire::{
    layer_key_kind, layer_key_owner, layer_slot, LAYER_KIND_BODY,
};

/// Where the last present drew an object's body (RFC-0112 Phase 3): the ghost's place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Place {
    pub dst_x: u16,
    pub dst_y: u16,
    pub w: u16,
    pub h: u16,
    /// The slot that frame read.
    pub slot: u8,
    /// The body's index in the frame's draw order (back to front).
    pub z: u8,
    /// The window's wire key in that compositor (`0xFFFF`: the desktop).
    pub window_key: u16,
}

/// An object's geometry: `slots` slots of `rows` rows at `stride` bytes; `width` the widest
/// content a slot holds (its size class).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Geometry {
    pub stride: u32,
    pub width: u16,
    pub rows: u16,
    pub slots: u8,
}

impl Geometry {
    /// What `r` is allocated as: its size class, at the stride that holds it.
    pub(crate) fn for_request(r: &AllocRequest) -> Self {
        let (width, rows) = wire::size_class(r.width, r.rows);
        Self { stride: wire::stride_for(width), width, rows, slots: r.slots }
    }

    /// The object's bytes: every slot, whole pages.
    pub(crate) const fn len(&self) -> usize {
        wire::object_bytes(self.stride, self.rows, self.slots)
    }

    /// Slot `slot`'s first byte.
    pub(crate) const fn slot_offset(&self, slot: u8) -> usize {
        wire::slot_offset(self.stride, self.rows, slot)
    }

    /// Whether `w`×`h` at column `x`, row `row` of slot `slot` lies inside the object.
    pub(crate) fn admits(&self, slot: u8, x: u32, row: u32, w: u32, h: u32) -> bool {
        slot < self.slots
            && x.checked_add(w).is_some_and(|end| end <= u32::from(self.width))
            && row.checked_add(h).is_some_and(|end| end <= u32::from(self.rows))
    }
}

/// One object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Surface {
    /// Its id in windowd's context: a layer's `source` (never 0).
    pub buffer: u32,
    /// The app it is for, by kernel id: who is charged.
    pub owner: u64,
    /// gpud's own capability to it.
    pub vmo: u32,
    pub geometry: Geometry,
    /// windowd freed it: unbound and closed once the present of this seq retired.
    freeing_after: Option<u32>,
    /// Where the last present drew its body (`None`: no frame shows it — minimized, hidden).
    place: Option<Place>,
    /// Its compositor died (RFC-0112): kept for the next one to adopt, composed by nobody until
    /// then.
    orphan: bool,
}

impl Surface {
    /// Where the last present drew its body.
    pub(crate) const fn place(&self) -> Option<Place> {
        self.place
    }
}

/// Every object gpud holds for an app.
pub(crate) struct Surfaces {
    live: Vec<Surface>,
    next: u32,
}

/// `a` is at or after `b` on the present sequence (which wraps).
const fn at_or_after(a: u32, b: u32) -> bool {
    a.wrapping_sub(b) < u32::MAX / 2
}

impl Surfaces {
    pub(crate) const fn new() -> Self {
        Self { live: Vec::new(), next: 1 }
    }

    /// Bytes every object spans.
    pub(crate) fn bytes(&self) -> u64 {
        self.live.iter().map(|s| s.geometry.len() as u64).fold(0, u64::saturating_add)
    }

    /// How many apps hold an object.
    pub(crate) fn owners(&self) -> usize {
        let mut seen: Vec<u64> = Vec::new();
        for s in &self.live {
            if !seen.contains(&s.owner) && seen.try_reserve(1).is_ok() {
                seen.push(s.owner);
            }
        }
        seen.len()
    }

    /// Bytes `owner`'s objects span (its charge).
    pub(crate) fn owned_by(&self, owner: u64) -> u64 {
        let mine = self.live.iter().filter(|s| s.owner == owner);
        mine.map(|s| s.geometry.len() as u64).fold(0, u64::saturating_add)
    }

    /// Whether one more object of `len` bytes keeps every object within `ceiling`.
    pub(crate) fn admits_more(&self, len: usize, ceiling: u64) -> bool {
        self.bytes().saturating_add(len as u64) <= ceiling
    }

    /// An id never 0 and never a live object's.
    fn fresh_id(&mut self) -> u32 {
        loop {
            let id = self.next;
            self.next = self.next.wrapping_add(1).max(1);
            if self.live.iter().all(|s| s.buffer != id) {
                return id;
            }
        }
    }

    /// The object `vmo` of `geometry` for `owner`: its id. `None` when the table cannot grow.
    pub(crate) fn insert(&mut self, owner: u64, vmo: u32, geometry: Geometry) -> Option<u32> {
        self.live.try_reserve(1).ok()?;
        let buffer = self.fresh_id();
        let s = Surface {
            buffer,
            owner,
            vmo,
            geometry,
            freeing_after: None,
            place: None,
            orphan: false,
        };
        self.live.push(s);
        Some(buffer)
    }

    /// The live object `buffer` (a freed one still being read is live until it retires).
    pub(crate) fn get(&self, buffer: u32) -> Option<&Surface> {
        self.live.iter().find(|s| s.buffer == buffer)
    }

    /// The object `buffer` taken out at once (an allocation rolled back: no frame read it).
    pub(crate) fn remove(&mut self, buffer: u32) -> Option<Surface> {
        let i = self.live.iter().position(|s| s.buffer == buffer)?;
        Some(self.live.swap_remove(i))
    }

    /// `OP_SURFACE_FREE`: windowd composes `buffer` no more — frames up to present `seq` may
    /// still read it. False: no such object, or freed already.
    pub(crate) fn free_after(&mut self, buffer: u32, seq: u32) -> bool {
        match self.live.iter_mut().find(|s| s.buffer == buffer && s.freeing_after.is_none()) {
            Some(s) => {
                s.freeing_after = Some(seq);
                true
            }
            None => false,
        }
    }

    /// The present `seq` retired: every freed object no later frame can read leaves the table,
    /// handed to `gone` (the caller unbinds it from its backend, closes it, ends the charge).
    pub(crate) fn retire(&mut self, seq: u32, gone: &mut dyn FnMut(Surface)) {
        let mut i = 0;
        while i < self.live.len() {
            if self.live[i].freeing_after.is_some_and(|after| at_or_after(seq, after)) {
                gone(self.live.swap_remove(i));
            } else {
                i += 1;
            }
        }
    }

    /// A present came in: where it draws each object's body — a `CompositeLayer` of the body
    /// kind naming the object — in draw order; an object no body of this present names is shown
    /// by no frame from now on (its window minimized, hidden, or gone).
    pub(crate) fn note_places(&mut self, cmds: &[Command]) {
        for s in &mut self.live {
            s.place = None;
        }
        let mut z = 0u8;
        for c in cmds {
            let Command::CompositeLayer {
                source,
                flags,
                dst_x,
                dst_y,
                width,
                height,
                content_w,
                content_h,
                key,
                ..
            } = c
            else {
                continue;
            };
            if *source == 0 || layer_key_kind(*key) != LAYER_KIND_BODY {
                continue;
            }
            let Some(s) = self.live.iter_mut().find(|s| s.buffer == *source) else { continue };
            let (w, h) = (
                if *content_w > 0 { *content_w } else { *width },
                if *content_h > 0 { *content_h } else { *height },
            );
            let fits = |v: u32| u16::try_from(v).ok();
            if let (Some(dst_x), Some(dst_y), Some(w), Some(h)) =
                (fits(*dst_x), fits(*dst_y), fits(w), fits(h))
            {
                let window_key = layer_key_owner(*key);
                s.place =
                    Some(Place { dst_x, dst_y, w, h, slot: layer_slot(*flags), z, window_key });
            }
            z = z.saturating_add(1);
        }
    }

    /// A new compositor runs (RFC-0112 Phase 3): no frame of the old one is presented again.
    /// What the last compositor left unadopted, and every object being freed, leaves the table
    /// now (handed to `gone`); every other object is kept as an orphan for the new compositor to
    /// adopt — with the place its body had. Returns how many are kept.
    pub(crate) fn orphan_all(&mut self, gone: &mut dyn FnMut(Surface)) -> usize {
        let mut i = 0;
        while i < self.live.len() {
            if self.live[i].orphan || self.live[i].freeing_after.is_some() {
                gone(self.live.swap_remove(i));
            } else {
                self.live[i].orphan = true;
                i += 1;
            }
        }
        self.live.len()
    }

    /// The orphans no frame showed leave the table now (handed to `gone`): a window that was
    /// minimized or hidden has no ghost.
    pub(crate) fn release_unplaced_orphans(&mut self, gone: &mut dyn FnMut(Surface)) {
        let mut i = 0;
        while i < self.live.len() {
            if self.live[i].orphan && self.live[i].place.is_none() {
                gone(self.live.swap_remove(i));
            } else {
                i += 1;
            }
        }
    }

    /// The next orphan a frame showed, for the new compositor to adopt.
    pub(crate) fn next_orphan(&self) -> Option<Surface> {
        self.live.iter().find(|s| s.orphan && s.place.is_some()).copied()
    }

    /// Orphans a frame showed that are not adopted yet.
    pub(crate) fn orphans_placed(&self) -> usize {
        self.live.iter().filter(|s| s.orphan && s.place.is_some()).count()
    }

    /// The new compositor adopted `buffer`: composed again, by it.
    pub(crate) fn adopted(&mut self, buffer: u32) {
        if let Some(s) = self.live.iter_mut().find(|s| s.buffer == buffer) {
            s.orphan = false;
        }
    }

    /// Whether every layer of a present that samples an object names a live one — not one being
    /// freed, not an orphan the compositor has not adopted — and lies inside it (its sample and
    /// its scroll band): the GPU never reads past an object, a layer never names another
    /// context's id.
    pub(crate) fn admits_commands(&self, cmds: &[Command]) -> bool {
        cmds.iter().all(|c| match c {
            Command::CompositeLayer {
                source,
                flags,
                src_x,
                src_row_abs,
                width,
                height,
                content_w,
                content_h,
                scroll_band_top_abs,
                scroll_band_h,
                ..
            } if *source != 0 => {
                let live = |s: &&Surface| s.freeing_after.is_none() && !s.orphan;
                let Some(s) = self.get(*source).filter(live) else {
                    return false;
                };
                let (w, h) = (
                    if *content_w > 0 { *content_w } else { *width },
                    if *content_h > 0 { *content_h } else { *height },
                );
                let slot = layer_slot(*flags);
                let g = &s.geometry;
                g.admits(slot, *src_x, *src_row_abs, w, h)
                    && (*scroll_band_h == 0
                        || g.admits(slot, *src_x, *scroll_band_top_abs, w, *scroll_band_h))
            }
            _ => true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER: u64 = 0xA11CE;

    fn request(width: u16, rows: u16) -> AllocRequest {
        AllocRequest { cookie: 1, owner_sid: OWNER, width, rows, slots: 2, usage: 1 }
    }

    fn layer(source: u32, slot: u8, (x, row, w, h): (u32, u32, u32, u32)) -> Command {
        body(source, slot, (x, row, w, h), (0, 0), 1)
    }

    /// A window body of `key` at `dst` (the kind the key names decides whether it is a body).
    fn body(
        source: u32,
        slot: u8,
        (x, row, w, h): (u32, u32, u32, u32),
        (dst_x, dst_y): (u32, u32),
        key: u32,
    ) -> Command {
        Command::CompositeLayer {
            src_row_abs: row,
            src_x: x,
            width: w,
            height: h,
            dst_x,
            dst_y,
            opacity: 255,
            corner_radius: 0,
            shadow_blur: 0,
            shadow_offset_y: 0,
            shadow_alpha: 0,
            backdrop_blur: 0,
            content_w: 0,
            content_h: 0,
            scroll_band_top_abs: 0,
            scroll_band_h: 0,
            layer_id: 1,
            content_epoch: 1,
            flags: nexus_gfx::command::layer_wire::with_slot(0, slot),
            key,
            source,
        }
    }

    fn body_key(window: u32) -> u32 {
        nexus_gfx::command::layer_wire::layer_key(window, LAYER_KIND_BODY, 0)
    }

    #[test]
    fn an_object_is_its_size_class_and_its_ids_are_never_zero_nor_reused_live() {
        let g = Geometry::for_request(&request(1440, 2389));
        assert_eq!((g.width, g.rows, g.stride, g.slots), (1472, 2432, 5888, 2));
        assert_eq!(g.len() % 4096, 0);
        assert_eq!(g.slot_offset(1), 2432 * 5888);
        let mut t = Surfaces::new();
        let a = t.insert(OWNER, 10, g).expect("a");
        let b = t.insert(OWNER, 11, g).expect("b");
        assert!(a != 0 && b != 0 && a != b);
        assert_eq!(t.owned_by(OWNER), 2 * g.len() as u64);
        assert_eq!(t.owned_by(OWNER + 1), 0);
    }

    #[test]
    fn a_freed_object_leaves_once_its_last_frame_retired() {
        let g = Geometry::for_request(&request(640, 480));
        let mut t = Surfaces::new();
        let a = t.insert(OWNER, 10, g).expect("a");
        assert!(t.free_after(a, 41));
        assert!(!t.free_after(a, 42), "freed once");
        let mut gone = Vec::new();
        t.retire(40, &mut |s| gone.push(s.buffer));
        assert!(gone.is_empty(), "frame 41 may still read it");
        t.retire(41, &mut |s| gone.push(s.buffer));
        assert_eq!(gone, [a]);
        assert!(t.get(a).is_none());
        // Across the sequence's wrap.
        let b = t.insert(OWNER, 12, g).expect("b");
        assert!(t.free_after(b, u32::MAX));
        t.retire(3, &mut |s| gone.push(s.buffer));
        assert_eq!(gone, [a, b]);
    }

    #[test]
    fn test_reject_a_layer_past_its_object_or_of_no_live_object() {
        let g = Geometry::for_request(&request(640, 480));
        let mut t = Surfaces::new();
        let a = t.insert(OWNER, 10, g).expect("a");
        assert!(t.admits_commands(&[layer(a, 1, (0, 0, 640, 480))]));
        assert!(t.admits_commands(&[layer(0, 0, (0, 9000, 1920, 1080))]), "layout layers pass");
        assert!(!t.admits_commands(&[layer(a, 2, (0, 0, 64, 64))]), "slot 2 of two");
        assert!(!t.admits_commands(&[layer(a, 0, (1, 0, 640, 64))]), "one column past");
        let rows = u32::from(g.rows);
        assert!(t.admits_commands(&[layer(a, 0, (0, 1, 64, rows - 1))]), "the class's own rows");
        assert!(!t.admits_commands(&[layer(a, 0, (0, 1, 64, rows))]), "one row past the class");
        assert!(!t.admits_commands(&[layer(a + 1, 0, (0, 0, 64, 64))]), "no such object");
        assert!(!t.admits_commands(&[layer(a, 0, (0, u32::MAX, 64, 64))]), "no wrap");
        assert!(t.free_after(a, 7));
        assert!(!t.admits_commands(&[layer(a, 0, (0, 0, 64, 64))]), "freed: named no more");
    }

    /// An allocation its backend refused leaves at once: no charge, no id left behind.
    #[test]
    fn a_rolled_back_allocation_leaves_nothing() {
        let g = Geometry::for_request(&request(640, 480));
        let mut t = Surfaces::new();
        let a = t.insert(OWNER, 10, g).expect("a");
        assert_eq!(t.remove(a).map(|s| s.vmo), Some(10));
        assert_eq!(t.owned_by(OWNER), 0);
        assert!(t.remove(a).is_none(), "once");
    }

    #[test]
    fn test_reject_an_object_past_the_ceiling() {
        let g = Geometry::for_request(&request(1920, 3072));
        let mut t = Surfaces::new();
        let ceiling = 2 * g.len() as u64;
        assert!(t.admits_more(g.len(), ceiling));
        t.insert(OWNER, 10, g).expect("a");
        assert!(t.admits_more(g.len(), ceiling));
        t.insert(OWNER, 11, g).expect("b");
        assert!(!t.admits_more(g.len(), ceiling));
    }

    /// RFC-0112 Phase 3: a present places each object's body (draw order = z); a body of the
    /// desktop carries the desktop key; a region layer places nothing; an object the next present
    /// does not show loses its place.
    #[test]
    fn a_present_places_every_body_it_draws_and_unplaces_the_rest() {
        let g = Geometry::for_request(&request(640, 480));
        let mut t = Surfaces::new();
        let a = t.insert(OWNER, 10, g).expect("a");
        let b = t.insert(OWNER + 1, 11, g).expect("b");
        let desk = t.insert(OWNER + 2, 12, g).expect("desk");
        let region = nexus_gfx::command::layer_wire::layer_key(7, 2, 0);
        t.note_places(&[
            body(desk, 0, (0, 0, 640, 480), (0, 0), body_key(0xFFFF)),
            body(a, 1, (0, 0, 640, 480), (100, 40), body_key(7)),
            body(a, 1, (0, 0, 64, 64), (100, 40), region),
            body(b, 0, (0, 0, 320, 240), (700, 300), body_key(9)),
        ]);
        let pa = t.get(a).and_then(Surface::place).expect("a placed");
        assert_eq!(
            (pa.dst_x, pa.dst_y, pa.w, pa.h, pa.slot, pa.z, pa.window_key),
            (100, 40, 640, 480, 1, 1, 7)
        );
        let pd = t.get(desk).and_then(Surface::place).expect("desk placed");
        assert_eq!((pd.z, pd.window_key), (0, 0xFFFF));
        assert_eq!(t.get(b).and_then(Surface::place).map(|p| (p.z, p.window_key)), Some((2, 9)));
        // b minimized: the next present draws a alone, spread over a wider frame.
        let mut spread = body(a, 1, (0, 0, 640, 480), (100, 40), body_key(7));
        if let Command::CompositeLayer { content_w, content_h, width, height, .. } = &mut spread {
            (*content_w, *content_h, *width, *height) = (640, 480, 800, 600);
        }
        t.note_places(&[spread]);
        assert_eq!(t.get(b).and_then(Surface::place), None, "no body drew b");
        assert_eq!(t.get(a).and_then(Surface::place).map(|p| (p.w, p.h)), Some((640, 480)));
    }

    /// RFC-0112 Phase 3: a new compositor keeps every placed object as an orphan — nothing of it
    /// is composed until adopted — and the unplaced orphans leave at the adopt; a second restart
    /// before any adopt releases what the first left.
    #[test]
    fn a_new_compositor_keeps_the_placed_objects_as_orphans_until_adopted() {
        let mut table = Surfaces::new();
        let g = Geometry::for_request(&request(64, 64));
        let a = table.insert(OWNER, 11, g).expect("insert");
        let b = table.insert(OWNER + 1, 12, g).expect("insert");
        let c = table.insert(OWNER + 1, 13, g).expect("insert");
        assert!(table.free_after(c, 7), "one is being freed already");
        table.note_places(&[body(a, 0, (0, 0, 64, 64), (10, 10), body_key(3))]);
        let mut gone = Vec::new();
        assert_eq!(table.orphan_all(&mut |s| gone.push(s.buffer)), 2);
        assert_eq!(gone, [c], "the freed one goes, the rest is kept");
        assert_eq!(table.owners(), 2, "two apps hold the kept objects");
        assert!(!table.admits_commands(&[layer(a, 0, (0, 0, 64, 64))]), "an orphan is nobody's");
        assert_eq!(table.orphans_placed(), 1);
        table.release_unplaced_orphans(&mut |s| gone.push(s.buffer));
        assert_eq!(gone, [c, b], "b was shown by no frame: no ghost");
        assert_eq!(table.next_orphan().map(|s| s.buffer), Some(a));
        table.adopted(a);
        assert_eq!((table.next_orphan(), table.orphans_placed()), (None, 0));
        assert!(table.admits_commands(&[layer(a, 0, (0, 0, 64, 64))]), "adopted: composed again");
        assert_eq!(table.owned_by(OWNER), g.len() as u64, "the charge stands");
        // A third restart with nothing adopted in between: the orphans of the second go.
        assert_eq!(table.orphan_all(&mut |s| gone.push(s.buffer)), 1);
        assert_eq!(table.orphan_all(&mut |s| gone.push(s.buffer)), 0);
        assert_eq!(gone, [c, b, a]);
        assert_eq!(table.bytes(), 0);
    }
}
