// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The ring of planes of the board's display (RFC-0101 §8, Phase 1 — TASK-0216): the
//! layout's three display planes as the slots of a [`PlaneRing`], one scanned by the
//! controller, one queued for the vertical blank, one free for the running present. A present
//! draws into the free slot, its flip is queued, and it is ACKED when the flip has landed at a
//! blank — the ack is windowd's frame clock. A present drawn while a flip is already queued
//! waits, drawn, for the next blank; a third present finds no free slot and is refused
//! (windowd keeps at most two in flight). The vertical-blank interrupt's counters live here
//! too: the first second's rate, the line the handler saw, the flips written late.
//! OWNERS: @gpu @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the ring's rules in `nexus_gfx::backend::v2::scanout` (host); this binding
//!   by the board ladder (`gpud: dc vblank ok (`, `gpud: dc flip ok (`) and the tear gate
//!   (`gpud: FAIL dc present front` forbidden)

use nexus_display_proto::layout;
use nexus_gfx::backend::types::Rect;
use nexus_gfx::backend::v2::scanout::{PlaneRing, RingError};
use nexus_gfx::backend::v2::TimelinePoint;

/// The ring's slots: the layout's three display planes.
pub(super) const SLOTS: usize = layout::RING_ROWS.len();
/// Each slot's first row in the layout.
pub(super) const ROWS: [u32; SLOTS] = layout::RING_ROWS;

/// A present drawn into a slot and waiting for its flip: the ack it earns once it is on
/// screen (`None` for a frame submitted outside a present — nothing to ack).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Drawn {
    pub(super) slot: usize,
    pub(super) ack: Option<(u32, u8)>,
}

/// The ring and the presents in flight on it.
pub(super) struct Ring {
    ring: PlaneRing<SLOTS>,
    /// The slot the running present draws into.
    drawing: Option<usize>,
    /// The present whose flip is queued for the next blank.
    queued: Option<Drawn>,
    /// A present drawn while a flip was queued: queued when that one lands.
    waiting: Option<Drawn>,
    /// Flips written at a blank.
    pub(super) flips: u32,
}

impl Ring {
    pub(super) fn new(width: u32, height: u32) -> Self {
        Self {
            ring: PlaneRing::new(width, height),
            drawing: None,
            queued: None,
            waiting: None,
            flips: 0,
        }
    }

    /// A slot for the present that begins; `Err` when every slot is scanned, queued or waiting.
    pub(super) fn begin(&mut self) -> Result<usize, RingError> {
        let slot = self.ring.acquire()?;
        self.drawing = Some(slot);
        Ok(slot)
    }

    /// The slot the running present draws into.
    pub(super) fn drawing(&self) -> Option<usize> {
        self.drawing
    }

    /// The layout row of the plane on screen, if one is.
    pub(super) fn scanned_row(&self) -> Option<u32> {
        self.ring.scanned().map(|s| ROWS[s])
    }

    /// The slot on screen.
    pub(super) fn scanned(&self) -> Option<usize> {
        self.ring.scanned()
    }

    /// The running present touched `damage` of its slot.
    pub(super) fn damage(&mut self, rect: Rect) {
        if let Some(slot) = self.drawing {
            // Past the list's bound the slot is redrawn whole on its next use; nothing is lost.
            let _ = self.ring.damage(slot, rect);
        }
    }

    /// The running present is drawn: its flip is queued for the next blank, or it waits behind
    /// the flip already queued. `ack` is attached by [`Self::attach_ack`] once the loop knows it.
    pub(super) fn drawn(&mut self) -> Result<(), RingError> {
        let slot = self.drawing.take().ok_or(RingError::NotRendering)?;
        let drawn = Drawn { slot, ack: None };
        match self.ring.flip(slot, TimelinePoint(self.flips as u64 + 1)) {
            Ok(()) => {
                self.queued = Some(drawn);
                Ok(())
            }
            Err(RingError::FlipPending) if self.waiting.is_none() => {
                self.waiting = Some(drawn);
                Ok(())
            }
            Err(e) => {
                let _ = self.ring.release(slot);
                Err(e)
            }
        }
    }

    /// The running present is discarded (drawn while the splash holds the glass, or refused).
    pub(super) fn discard(&mut self) {
        if let Some(slot) = self.drawing.take() {
            let _ = self.ring.release(slot);
        }
    }

    /// The running present is on screen at once — the reveal's one immediate switch.
    pub(super) fn shown_now(&mut self) -> Result<(), RingError> {
        let slot = self.drawing.take().ok_or(RingError::NotRendering)?;
        self.ring.flip(slot, TimelinePoint(0))?;
        self.ring.vblank(|_| true);
        self.flips += 1;
        Ok(())
    }

    /// The ack the most recently drawn present earns when it lands.
    pub(super) fn attach_ack(&mut self, ack: (u32, u8)) -> bool {
        match self.waiting.as_mut().or(self.queued.as_mut()) {
            Some(drawn) if drawn.ack.is_none() => {
                drawn.ack = Some(ack);
                true
            }
            _ => false,
        }
    }

    /// The slot whose flip waits for the next blank.
    pub(super) fn flip_pending(&self) -> Option<usize> {
        self.queued.map(|d| d.slot)
    }

    /// A blank at which the queued flip was written: the present on screen now (its ack), and
    /// the waiting present's flip queued for the next blank.
    pub(super) fn landed(&mut self) -> Option<Drawn> {
        self.ring.vblank(|_| true)?;
        self.flips += 1;
        let landed = self.queued.take();
        if let Some(next) = self.waiting.take() {
            match self.ring.flip(next.slot, TimelinePoint(self.flips as u64 + 1)) {
                Ok(()) => self.queued = Some(next),
                Err(_) => {
                    let _ = self.ring.release(next.slot);
                }
            }
        }
        landed
    }

    /// A blank at which nothing was flipped (nothing queued, or the handler came late): the
    /// planes' ages advance.
    pub(super) fn passed(&mut self) {
        let _ = self.ring.vblank(|_| false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHOLE: Rect = Rect { x: 0, y: 0, width: 1920, height: 1080 };

    #[test]
    fn a_present_is_acked_at_the_blank_its_flip_lands_on_and_two_wait_in_order() {
        let mut ring = Ring::new(1920, 1080);
        let a = ring.begin().unwrap();
        ring.damage(WHOLE);
        ring.drawn().unwrap();
        assert!(ring.attach_ack((1, 0)));
        assert_eq!(ring.flip_pending(), Some(a));
        let b = ring.begin().unwrap();
        assert_ne!(a, b);
        ring.drawn().unwrap();
        assert!(ring.attach_ack((2, 0)));
        assert_eq!(ring.flip_pending(), Some(a), "the second waits behind the first");
        let c = ring.begin().unwrap();
        ring.drawn().unwrap();
        assert!(ring.attach_ack((3, 0)));
        assert!(ring.begin().is_err(), "three in flight: no slot is free");
        assert_eq!(ring.landed(), Some(Drawn { slot: a, ack: Some((1, 0)) }));
        assert_eq!(ring.scanned(), Some(a));
        assert_eq!(ring.flip_pending(), Some(b));
        assert_eq!(ring.landed(), Some(Drawn { slot: b, ack: Some((2, 0)) }));
        assert_eq!(ring.flip_pending(), Some(c));
        assert_eq!(ring.landed().and_then(|d| d.ack), Some((3, 0)));
        assert_eq!(ring.landed(), None, "nothing queued: nothing lands");
        assert_eq!(ring.begin(), Ok(b), "the plane shown a frame ago is free again");
    }

    #[test]
    fn a_frame_under_the_splash_is_discarded_and_the_reveal_shows_at_once() {
        let mut ring = Ring::new(8, 8);
        ring.begin().unwrap();
        ring.discard();
        assert_eq!(ring.flip_pending(), None);
        let a = ring.begin().unwrap();
        ring.shown_now().unwrap();
        assert_eq!(ring.scanned(), Some(a));
        assert_eq!(ring.scanned_row(), Some(ROWS[a]));
        assert!(!ring.attach_ack((1, 0)), "nothing in flight takes an ack");
    }

    #[test]
    fn test_reject_drawn_without_a_present() {
        let mut ring = Ring::new(8, 8);
        assert_eq!(ring.drawn(), Err(RingError::NotRendering));
        assert_eq!(ring.shown_now(), Err(RingError::NotRendering));
    }
}
