// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The ring of planes of the board's display (RFC-0101 §8, Phase 1 — TASK-0216; flips
//! through the controller's latch — TASK-0342): the layout's three display planes as the slots
//! of a [`PlaneRing`] — one scanned by the controller, one LATCHED (its address written with the
//! configuration latch; the controller takes it at its next frame boundary), one free for the
//! running present. A drawn present is latched at once when no flip is; one drawn while a flip is
//! latched waits, drawn, and is latched the moment that flip lands; a third finds no free slot
//! and is refused (windowd keeps at most two in flight). A present is ACKED when its flip has
//! landed — the ack is windowd's frame clock. Compiled for the host's tests too (`dc` is the
//! board's alone): until TASK-0342 these rules lived in `dc/` and their tests never ran.
//! OWNERS: @gpu @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the ring's rules below (host) and the plane ring's in
//!   `nexus_gfx::backend::v2::scanout`; the binding by the board ladder (`gpud: dc vblank ok (`,
//!   `gpud: dc flip ok (`) and the tear gate (`gpud: FAIL dc present front` forbidden)

use nexus_display_proto::layout;
use nexus_gfx::backend::types::Rect;
use nexus_gfx::backend::v2::scanout::{PlaneRing, RingError};
use nexus_gfx::backend::v2::TimelinePoint;

/// The ring's slots: the layout's three display planes.
pub(super) const SLOTS: usize = layout::RING_ROWS.len();
/// Each slot's first row in the layout.
pub(super) const ROWS: [u32; SLOTS] = layout::RING_ROWS;

/// A present drawn into a slot and on its way to the screen: the ack it earns once it is on
/// screen (`None` for a frame submitted outside a present — nothing to ack), and the times its
/// hops are measured from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Drawn {
    pub(super) slot: usize,
    pub(super) ack: Option<(u32, u8)>,
    pub(super) stamps: Stamps,
}

/// When a present was received and when it was drawn (its flip latched or waiting) — the frame
/// chain's hops in gpud (RFC-0108 §hops): received → drawn, drawn → on screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Stamps {
    pub(super) received_ns: u64,
    pub(super) drawn_ns: u64,
}

/// A flip that landed: the present now on screen, and the slot to latch next (the present that
/// waited behind it) — the caller writes that flip at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Landed {
    pub(super) drawn: Drawn,
    pub(super) next: Option<usize>,
}

/// The ring and the presents in flight on it.
pub(super) struct Ring {
    ring: PlaneRing<SLOTS>,
    /// The slot the running present draws into.
    drawing: Option<usize>,
    /// The present whose flip is latched (written; the controller has not taken it yet).
    latched: Option<Drawn>,
    /// A present drawn while a flip was latched: latched when that one lands.
    waiting: Option<Drawn>,
    /// Flips that landed.
    pub(super) flips: u32,
}

impl Ring {
    pub(super) fn new(width: u32, height: u32) -> Self {
        Self {
            ring: PlaneRing::new(width, height),
            drawing: None,
            latched: None,
            waiting: None,
            flips: 0,
        }
    }

    /// A slot for the present that begins; `Err` when every slot is scanned, latched or waiting.
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

    /// The running present is drawn: `Some(slot)` — no flip was latched, the caller latches
    /// this one now; `None` — it waits behind the latched flip. `ack` is attached by
    /// [`Self::attach_ack`] once the loop knows it.
    pub(super) fn drawn(&mut self) -> Result<Option<usize>, RingError> {
        let slot = self.drawing.take().ok_or(RingError::NotRendering)?;
        let drawn = Drawn { slot, ack: None, stamps: Stamps::default() };
        match self.ring.flip(slot, TimelinePoint(u64::from(self.flips) + 1)) {
            Ok(()) => {
                self.latched = Some(drawn);
                Ok(Some(slot))
            }
            Err(RingError::FlipPending) if self.waiting.is_none() => {
                self.waiting = Some(drawn);
                Ok(None)
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

    /// The running present is on screen at once — the reveal's one immediate switch (and a
    /// display without the flip's interrupt).
    pub(super) fn shown_now(&mut self) -> Result<(), RingError> {
        let slot = self.drawing.take().ok_or(RingError::NotRendering)?;
        self.ring.flip(slot, TimelinePoint(0))?;
        self.ring.vblank(|_| true);
        self.flips += 1;
        Ok(())
    }

    /// The ack the most recently drawn present earns when it lands, with its hops' times.
    pub(super) fn attach_ack(&mut self, ack: (u32, u8), stamps: Stamps) -> bool {
        match self.waiting.as_mut().or(self.latched.as_mut()) {
            Some(drawn) if drawn.ack.is_none() => {
                drawn.ack = Some(ack);
                drawn.stamps = stamps;
                true
            }
            _ => false,
        }
    }

    /// The slot whose flip is latched.
    pub(super) fn latched(&self) -> Option<usize> {
        self.latched.map(|d| d.slot)
    }

    /// The controller took the latched flip: its present is on screen (its ack), the plane shown
    /// before is free, and the waiting present is the next to latch. `None`: nothing was latched.
    pub(super) fn landed(&mut self) -> Option<Landed> {
        self.ring.vblank(|_| true)?;
        self.flips += 1;
        let drawn = self.latched.take()?;
        let mut next = None;
        if let Some(waiting) = self.waiting.take() {
            match self.ring.flip(waiting.slot, TimelinePoint(u64::from(self.flips) + 1)) {
                Ok(()) => {
                    next = Some(waiting.slot);
                    self.latched = Some(waiting);
                }
                Err(_) => {
                    let _ = self.ring.release(waiting.slot);
                }
            }
        }
        Some(Landed { drawn, next })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHOLE: Rect = Rect { x: 0, y: 0, width: 1920, height: 1080 };

    fn stamps(n: u64) -> Stamps {
        Stamps { received_ns: n, drawn_ns: n + 1 }
    }

    #[test]
    fn a_drawn_present_is_latched_at_once_and_acked_when_it_lands() {
        let mut ring = Ring::new(1920, 1080);
        let a = ring.begin().unwrap();
        ring.damage(WHOLE);
        assert_eq!(ring.drawn(), Ok(Some(a)), "no flip latched: this one is latched now");
        assert!(ring.attach_ack((1, 0), stamps(10)));
        assert_eq!(ring.latched(), Some(a));
        let landed = ring.landed().unwrap();
        assert_eq!(landed.drawn, Drawn { slot: a, ack: Some((1, 0)), stamps: stamps(10) });
        let s = landed.drawn.stamps;
        assert_eq!((s.received_ns, s.drawn_ns), (10, 11), "the hops' times ride to the landing");
        assert_eq!(landed.next, None);
        assert_eq!(ring.scanned(), Some(a));
        assert_eq!(ring.latched(), None);
    }

    #[test]
    fn a_present_drawn_behind_a_latched_flip_waits_and_is_latched_when_that_one_lands() {
        let mut ring = Ring::new(1920, 1080);
        let a = ring.begin().unwrap();
        assert_eq!(ring.drawn(), Ok(Some(a)));
        assert!(ring.attach_ack((1, 0), stamps(10)));
        let b = ring.begin().unwrap();
        assert_ne!(a, b);
        assert_eq!(ring.drawn(), Ok(None), "the second waits behind the latched first");
        assert!(ring.attach_ack((2, 0), stamps(20)));
        assert_eq!(ring.latched(), Some(a));
        let landed = ring.landed().unwrap();
        assert_eq!(landed.drawn.ack, Some((1, 0)));
        assert_eq!(landed.next, Some(b), "the waiting one is latched next");
        assert_eq!(ring.latched(), Some(b));
        let landed = ring.landed().unwrap();
        assert_eq!(landed.drawn, Drawn { slot: b, ack: Some((2, 0)), stamps: stamps(20) });
        assert_eq!(landed.next, None);
        assert_eq!(ring.begin(), Ok(a), "the plane shown a flip ago is free again");
    }

    #[test]
    fn test_reject_a_third_present_in_flight() {
        let mut ring = Ring::new(1920, 1080);
        ring.begin().unwrap();
        ring.drawn().unwrap();
        ring.begin().unwrap();
        ring.drawn().unwrap();
        ring.begin().unwrap();
        assert_eq!(ring.drawn(), Err(RingError::FlipPending), "one latched, one waiting");
        assert_eq!(ring.drawing(), None, "the refused present's slot is given back");
    }

    #[test]
    fn test_reject_a_landing_with_nothing_latched() {
        let mut ring = Ring::new(8, 8);
        assert_eq!(ring.landed(), None);
        ring.begin().unwrap();
        assert_eq!(ring.landed(), None, "drawing is not latched");
    }

    #[test]
    fn a_frame_under_the_splash_is_discarded_and_the_reveal_shows_at_once() {
        let mut ring = Ring::new(8, 8);
        ring.begin().unwrap();
        ring.discard();
        assert_eq!(ring.latched(), None);
        let a = ring.begin().unwrap();
        ring.shown_now().unwrap();
        assert_eq!(ring.scanned(), Some(a));
        assert_eq!(ring.scanned_row(), Some(ROWS[a]));
        assert!(!ring.attach_ack((1, 0), Stamps::default()), "nothing in flight takes an ack");
    }

    #[test]
    fn test_reject_drawn_without_a_present() {
        let mut ring = Ring::new(8, 8);
        assert_eq!(ring.drawn(), Err(RingError::NotRendering));
        assert_eq!(ring.shown_now(), Err(RingError::NotRendering));
    }
}
