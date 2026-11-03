// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The client surfaces windowd composes (ADR-0042; by reference since RFC-0114) — the
//! pure, host-tested state machine: the surface table (it grows with the surfaces; ids are never
//! reused), create validation, the surface's BUFFER once gpud made it, strict seq flow control
//! (one present in flight per surface), damage clamped to the surface's rows, and the SLOT
//! LEDGER — which slot the frames read and when a present's ack, the release, is due. No pixel
//! moves here or anywhere in windowd: a frame names the surface's buffer and slot, and the GPU
//! reads the app's memory.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests below (`surface_release_*`, the bounds, `test_reject_*`)
//! ADR: docs/adr/0092-a-surface-is-a-buffer-the-app-draws-and-the-gpu-reads-gpud-allocates-it-for-its-owner-windowd-composes-by-reference.md

extern crate alloc;

use alloc::vec::Vec;

use nexus_display_proto::client_surface::{
    DamageRect, FORMAT_BGRA8888, INPUT_KIND_RELEASE, INPUT_KIND_TAP, INPUT_KIND_WHEEL,
    MAX_DAMAGE_RECTS, SURFACE_STATUS_BAD_SEQ, SURFACE_STATUS_BAD_SURFACE, SURFACE_STATUS_MALFORMED,
    SURFACE_STATUS_QUOTA,
};
use nexus_display_proto::surface_alloc::SLOTS_MAX;

/// What a frame is to the client it goes to (TASK-0306, ADR-0075 — TASK-0339).
///
/// No class waits on the client any more: windowd sends non-blocking and what does not fit
/// waits in the client's bounded outbox (`crate::client_outbox`), leaving in order as the client
/// drains — the kernel says when (an armed writable waitset member). The class decides only
/// what may be MERGED while it waits. Before this, `Blocking`/`Critical` frames parked the
/// compositor on the client's queue: an app rendering its first frame for half a second froze
/// every window for that half second (cycle 54: input waits of 0.3–1.5 s at the launches).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Delivery {
    /// The client waits for it: a present, create or destroy ack. Ordered, never merged, never
    /// dropped while the client lives (a lost ack froze the client — the 2026-07-26 report).
    Ack,
    /// User intent with no recovery path: a tap, a release, a wheel step, a key, a text commit,
    /// a settings push. Ordered, never merged (nothing downstream reconstructs it).
    Intent,
    /// State the next frame supersedes: the pointer over the window, a drag's position, a frame
    /// pulse, a presentation snapshot. While the client is behind, the newest frame of its kind
    /// replaces the older one — never a backlog of stale positions.
    Latest,
}

impl Delivery {
    /// Whether frames of this class are kept in order and never merged.
    pub(crate) const fn ordered(self) -> bool {
        match self {
            Delivery::Ack | Delivery::Intent => true,
            Delivery::Latest => false,
        }
    }

    /// The class an input kind belongs to. A tap and a press's release are discrete — nothing
    /// downstream can reconstruct them (a lost release leaves a drag without its end, RFC-0095);
    /// a wheel step is a delta — merged away it would lose distance. Hover and drag motion are
    /// where things are NOW: the newest supersedes the older.
    pub(crate) const fn for_input(kind: u8) -> Self {
        match kind {
            INPUT_KIND_TAP | INPUT_KIND_RELEASE | INPUT_KIND_WHEEL => Delivery::Intent,
            _ => Delivery::Latest,
        }
    }
}

/// Bounds for app surfaces: the layout's size, so an app can go TRUE fullscreen (the "□" toggle
/// re-creates its surface at display size — `wm::toggle_fullscreen`). A validation ceiling only:
/// a surface's buffer is made at its own size (gpud's size class, RFC-0114 §1).
pub const MAX_SURFACE_W: u16 = nexus_display_proto::layout::LAYOUT_MAX.0 as u16;
/// A surface's visible height. Its ROWS (base + store, RFC-0109) are bounded by
/// `nexus_display_proto::surface_layers::MAX_SURFACE_ROWS`.
pub const MAX_SURFACE_H: u16 = nexus_display_proto::layout::LAYOUT_MAX.1 as u16;
pub const MIN_SURFACE_DIM: u16 = 16;

/// A surface's buffer as windowd names it (RFC-0114 §1): gpud's object of `slots` slots, each
/// `rows` rows of `stride` bytes and at most `width` pixels wide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceBuffer {
    /// gpud's id of the object in windowd's context — a layer's `source` (never 0).
    pub source: u32,
    pub stride: u32,
    pub width: u16,
    pub rows: u16,
    pub slots: u8,
    /// The cache line the app cleans its writes in (`0`: the GPU snoops the caches) — the
    /// app's to know, carried with the create's answer.
    pub cache_block: u16,
}

/// What a frame draws of a surface: its buffer, the slot of its newest present, and that
/// present's seq — the content's generation (a layer's `content_epoch`, RFC-0114 §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shown {
    pub buffer: SurfaceBuffer,
    pub slot: u8,
    pub generation: u32,
}

/// `frame` is at or before `retired` on the present sequence (which wraps).
const fn done(frame: u32, retired: u32) -> bool {
    retired.wrapping_sub(frame) < u32::MAX / 2
}

/// Which frames read which slot, and the present whose ack waits (RFC-0114 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct SlotLedger {
    /// The slot of the newest accepted present: the slot the frames read.
    latest: Option<u8>,
    /// Per slot, the last frame (windowd's present to gpud) that read it.
    read_by: [Option<u32>; SLOTS_MAX as usize],
    /// The present whose ack waits until every other slot is free, and when it came.
    owed: Option<(u32, u64)>,
}

impl SlotLedger {
    /// Every slot but the newest is read by no frame still in flight: the app may draw into any
    /// of them.
    fn others_free(&self, slots: u8, retired: u32) -> bool {
        (0..slots)
            .filter(|k| self.latest != Some(*k))
            .all(|k| self.read_by[usize::from(k)].is_none_or(|f| done(f, retired)))
    }
}

/// One live client surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientSurface {
    pub id: u32,
    pub width: u16,
    pub height: u16,
    /// The surface's rows (RFC-0109): `[0, height)` the base layer, `[height, rows)` the
    /// client's store (its scroll layers' bands, its wash patches).
    pub rows: u16,
    /// The store's used width: its rows' columns `[0, store_w)` (`0`: no store).
    pub store_w: u16,
    /// The surface's buffer, once gpud made it (`None`: the create waits for gpud's answer).
    pub buffer: Option<SurfaceBuffer>,
    /// windowd's capability to the buffer until the app holds it (moved with
    /// `OP_SURFACE_BUFFERS`).
    pub buffer_cap: Option<u32>,
    /// Last accepted present sequence number (0 = none yet).
    pub last_seq: u32,
    /// The client dropped the surface (`SURFACE_DESTROY`): it takes no present, and its last
    /// frame shows until its successor's first present or the window's close.
    pub retiring: bool,
    ledger: SlotLedger,
}

impl ClientSurface {
    /// What a frame draws of it; `None` before its first present.
    #[must_use]
    pub fn shown(&self) -> Option<Shown> {
        Some(Shown { buffer: self.buffer?, slot: self.ledger.latest?, generation: self.last_seq })
    }
}

/// The surface table: every live client surface, addressed by id — it grows with the surfaces
/// (the limits are memory's, RFC-0115, never a slot count). Ids are monotonic and never 0, so a
/// stale id from a destroyed surface can never alias a new one.
#[derive(Debug, Default)]
pub struct ClientSurfaces {
    surfaces: Vec<ClientSurface>,
    next_id: u32,
}

impl ClientSurfaces {
    #[cfg(test)]
    #[must_use]
    pub fn new() -> Self {
        Self::starting_at(1)
    }

    /// A table whose first id is `first` (never 0). The compositor seeds it with the
    /// milliseconds since boot (RFC-0112): ids are handed out far slower than one a millisecond,
    /// so a restarted compositor never reuses an id its predecessor gave out — a stale present
    /// or a late buffer answer naming one finds no surface of a new client.
    #[must_use]
    pub fn starting_at(first: u32) -> Self {
        Self { surfaces: Vec::new(), next_id: first.max(1) }
    }

    /// The surface with `id`, if resident.
    #[must_use]
    pub fn get_by_id(&self, id: u32) -> Option<&ClientSurface> {
        self.surfaces.iter().find(|s| s.id == id)
    }

    fn get_mut(&mut self, id: u32) -> Option<&mut ClientSurface> {
        self.surfaces.iter_mut().find(|s| s.id == id)
    }

    /// Validates and registers a surface. Returns its id, or a wire status (`MALFORMED` on a bad
    /// format or bounds, `QUOTA` when the table cannot grow).
    pub fn create(
        &mut self,
        width: u16,
        height: u16,
        (rows, store_w): (u16, u16),
        format: u8,
    ) -> Result<u32, u8> {
        if format != FORMAT_BGRA8888 {
            return Err(SURFACE_STATUS_MALFORMED);
        }
        if width < MIN_SURFACE_DIM
            || height < MIN_SURFACE_DIM
            || width > MAX_SURFACE_W
            || height > MAX_SURFACE_H
            || rows < height
            || rows > nexus_display_proto::surface_layers::MAX_SURFACE_ROWS
            || store_w > width
            || (rows > height) != (store_w > 0)
        {
            return Err(SURFACE_STATUS_MALFORMED);
        }
        self.surfaces.try_reserve(1).map_err(|_| SURFACE_STATUS_QUOTA)?;
        let id = loop {
            let id = self.next_id;
            self.next_id = self.next_id.wrapping_add(1).max(1);
            if self.get_by_id(id).is_none() {
                break id;
            }
        };
        self.surfaces.push(ClientSurface {
            id,
            width,
            height,
            rows,
            store_w,
            buffer: None,
            buffer_cap: None,
            last_seq: 0,
            retiring: false,
            ledger: SlotLedger::default(),
        });
        Ok(id)
    }

    /// RFC-0112 Phase 3: a GHOST — a surface of the compositor before this one, adopted with
    /// its buffer and the slot its last frame read. It is retiring from the start (it takes no
    /// present, owes no ack) and shows that slot, `width` × `height` of its base, until its
    /// owner's new surface presents. Refused like a create is, and when the shown part lies past
    /// the buffer.
    pub fn adopt(
        &mut self,
        (width, height): (u16, u16),
        buffer: SurfaceBuffer,
        slot: u8,
    ) -> Result<u32, u8> {
        let fits = buffer.source != 0
            && (1..=SLOTS_MAX).contains(&buffer.slots)
            && slot < buffer.slots
            && width <= buffer.width
            && height <= buffer.rows
            && buffer.stride >= u32::from(buffer.width) * 4;
        if !fits {
            return Err(SURFACE_STATUS_MALFORMED);
        }
        let id = self.create(width, height, (height, 0), FORMAT_BGRA8888)?;
        let s = self.get_mut(id).ok_or(SURFACE_STATUS_BAD_SURFACE)?;
        s.buffer = Some(buffer);
        s.last_seq = 1;
        s.retiring = true;
        s.ledger.latest = Some(slot);
        Ok(id)
    }

    /// gpud made the surface's buffer (`cap`: windowd's capability to it, for the app). It must
    /// hold the surface — its width and rows, one to `SLOTS_MAX` slots at a stride that holds its
    /// width; a retiring surface, or one that has its buffer, takes none.
    pub fn bind_buffer(
        &mut self,
        id: u32,
        buffer: SurfaceBuffer,
        cap: Option<u32>,
    ) -> Result<(), u8> {
        let s = self.get_mut(id).filter(|s| !s.retiring && s.buffer.is_none());
        let s = s.ok_or(SURFACE_STATUS_BAD_SURFACE)?;
        let fits = buffer.source != 0
            && (1..=SLOTS_MAX).contains(&buffer.slots)
            && buffer.width >= s.width
            && buffer.rows >= s.rows
            && buffer.stride >= u32::from(buffer.width) * 4;
        if !fits {
            return Err(SURFACE_STATUS_MALFORMED);
        }
        s.buffer = Some(buffer);
        s.buffer_cap = cap;
        Ok(())
    }

    /// The surfaces whose buffer's capability still waits to move to their client.
    pub fn owed_caps(&self) -> impl Iterator<Item = u32> + '_ {
        self.surfaces.iter().filter(|s| s.buffer_cap.is_some()).map(|s| s.id)
    }

    /// windowd's capability to the surface's buffer, taken to move it to the app.
    pub fn take_buffer_cap(&mut self, id: u32) -> Option<u32> {
        self.get_mut(id)?.buffer_cap.take()
    }

    /// Validates a present: a live surface with its buffer, a slot of it, strictly the next seq
    /// (one in flight — the app waits for the ack). Returns the surface and the damage clamped
    /// to its rows — the base's and the store's (RFC-0109) — and its width (empty rects
    /// dropped). The slot is what the frames read from now on; the ack is owed from `now_ns`
    /// until every other slot is free (`take_releases`).
    pub fn present(
        &mut self,
        surface_id: u32,
        (seq, slot): (u32, u8),
        damage: &[DamageRect],
        now_ns: u64,
    ) -> Result<(ClientSurface, [DamageRect; MAX_DAMAGE_RECTS], usize), u8> {
        let surface = self.get_mut(surface_id).filter(|s| !s.retiring);
        let surface = surface.ok_or(SURFACE_STATUS_BAD_SURFACE)?;
        // No buffer yet: the client has nothing it could have drawn into.
        let buffer = surface.buffer.ok_or(SURFACE_STATUS_BAD_SURFACE)?;
        if slot >= buffer.slots {
            return Err(SURFACE_STATUS_MALFORMED);
        }
        if seq != surface.last_seq.wrapping_add(1) {
            return Err(SURFACE_STATUS_BAD_SEQ);
        }
        let mut clamped = [DamageRect { x: 0, y: 0, width: 0, height: 0 }; MAX_DAMAGE_RECTS];
        let mut count = 0usize;
        for rect in damage.iter().take(MAX_DAMAGE_RECTS) {
            if rect.x >= surface.width || rect.y >= surface.rows {
                continue;
            }
            let w = rect.width.min(surface.width - rect.x);
            let h = rect.height.min(surface.rows - rect.y);
            if w == 0 || h == 0 {
                continue;
            }
            clamped[count] = DamageRect { x: rect.x, y: rect.y, width: w, height: h };
            count += 1;
        }
        surface.last_seq = seq;
        surface.ledger.latest = Some(slot);
        surface.ledger.owed = Some((seq, now_ns));
        Ok((*surface, clamped, count))
    }

    /// The client dropped the surface (`SURFACE_DESTROY`): it takes no present from now on and
    /// owes no ack (the client re-creates or leaves), and its last frame shows until the runtime
    /// destroys it.
    pub fn retire(&mut self, surface_id: u32) -> Result<(), u8> {
        let s = self.get_mut(surface_id).filter(|s| !s.retiring);
        let s = s.ok_or(SURFACE_STATUS_BAD_SURFACE)?;
        s.retiring = true;
        s.ledger.owed = None;
        Ok(())
    }

    /// Removes the surface: returned for the runtime to free its buffer (and to close windowd's
    /// capability to it when the app never got it).
    pub fn destroy(&mut self, surface_id: u32) -> Result<ClientSurface, u8> {
        let i = self.surfaces.iter().position(|s| s.id == surface_id);
        let i = i.ok_or(SURFACE_STATUS_BAD_SURFACE)?;
        Ok(self.surfaces.swap_remove(i))
    }

    /// Frame `frame` (windowd's present to gpud) read the newest slot of each surface in `ids`.
    pub fn note_read(&mut self, ids: &[u32], frame: u32) {
        for s in self.surfaces.iter_mut().filter(|s| ids.contains(&s.id)) {
            if let Some(slot) = s.ledger.latest {
                s.ledger.read_by[usize::from(slot)] = Some(frame);
            }
        }
    }

    /// The present acks due now that every frame up to `retired` is read no more (RFC-0114 §5):
    /// `(surface, seq, owed since)` of each surface whose other slots are free, appended to `out`
    /// — each owed ack once.
    pub fn take_releases(&mut self, retired: u32, out: &mut Vec<(u32, u32, u64)>) {
        for s in self.surfaces.iter_mut() {
            let (Some((seq, since)), Some(buffer)) = (s.ledger.owed, s.buffer) else { continue };
            if s.ledger.others_free(buffer.slots, retired) {
                s.ledger.owed = None;
                out.push((s.id, seq, since));
            }
        }
    }
}

#[cfg(test)]
#[path = "client_surface_tests.rs"]
mod tests;

#[cfg(test)]
mod delivery_tests {
    use super::{Delivery, INPUT_KIND_RELEASE, INPUT_KIND_TAP};
    use nexus_display_proto::client_surface::{
        INPUT_KIND_DRAG, INPUT_KIND_LEAVE, INPUT_KIND_MOVE, INPUT_KIND_WHEEL,
    };

    /// TASK-0306 → ADR-0075: the bug was a POLICY bug, so the policy is what gets pinned. What
    /// the client waits for, and what the user meant, is kept whole and in order; only state
    /// may be merged while the client is behind. Nothing parks the compositor.
    #[test]
    fn acks_and_intent_are_ordered_state_is_merged() {
        assert!(Delivery::Ack.ordered(), "a client is waiting on this frame");
        assert!(Delivery::Intent.ordered(), "user intent has no recovery path");
        assert!(!Delivery::Latest.ordered(), "the next state supersedes this one");
    }

    /// A tap, a release and a wheel step are discrete: kept, in order. The pointer's move and
    /// leave and a drag's motion are state: the newest wins.
    #[test]
    fn the_input_kinds_split_into_intent_and_state() {
        for kind in [INPUT_KIND_TAP, INPUT_KIND_RELEASE, INPUT_KIND_WHEEL] {
            assert_eq!(Delivery::for_input(kind), Delivery::Intent, "kind {kind}");
        }
        for kind in [INPUT_KIND_MOVE, INPUT_KIND_LEAVE, INPUT_KIND_DRAG] {
            assert_eq!(Delivery::for_input(kind), Delivery::Latest, "kind {kind}");
        }
    }

    /// A merged wheel step would lose scroll distance; a merged release would leave a drag open.
    #[test]
    fn test_reject_merging_a_release_or_a_wheel_step() {
        assert!(Delivery::for_input(INPUT_KIND_RELEASE).ordered());
        assert!(Delivery::for_input(INPUT_KIND_WHEEL).ordered());
    }
}
