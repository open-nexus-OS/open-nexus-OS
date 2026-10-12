// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The scanout contract (RFC-0101 §8) between the compositor and the display, as
//! the model both are built against: a ring of planes the display scans one of, a flip that
//! names a slot, its damage and the point that says the drawing is done, and that takes
//! effect at the next vertical blank — never before; the vertical blank as the frame clock;
//! damage per plane (buffer age) so a client redraws exactly what changed since this slot
//! was last on screen; the pointer as the controller's own plane, moved without a flip and
//! whole at every border. Measured red states this turns (2026-10-10): every present drawn
//! into the scanned plane (`front` = `n`), shadows darkening with each present, the pointer's
//! sprite breaking apart at the right border.
//! OWNERS: @ui @gpu
//! STATUS: Functional (contract; the board's controller implements it in RFC-0101 Phase 1)
//! API_STABILITY: Unstable until RFC-0101 Phase 1
//! TEST_COVERAGE: unit tests (this file) — the reject matrix of RFC-0101 §9
//! INVARIANTS: the scanned slot is never a render target; at most one flip is queued; a
//!   flip takes effect only at a vertical blank and only once its point is reached.

use super::TimelinePoint;
use crate::backend::types::Rect;

/// Damage rects one slot remembers per frame.
pub const MAX_DAMAGE: usize = 8;

/// What a slot of the ring is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotState {
    Free,
    Rendering,
    Queued,
    Scanned,
}

/// One plane of the ring.
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    state: SlotState,
    /// Frames since this slot was last scanned; `None` if never.
    age: Option<u32>,
    /// The point that says the drawing into it is done.
    done: TimelinePoint,
    damage: [Rect; MAX_DAMAGE],
    damage_count: usize,
}

impl Slot {
    const EMPTY: Self = Self {
        state: SlotState::Free,
        age: None,
        done: TimelinePoint(0),
        damage: [Rect { x: 0, y: 0, width: 0, height: 0 }; MAX_DAMAGE],
        damage_count: 0,
    };

    pub const fn state(&self) -> SlotState {
        self.state
    }

    /// Buffer age: frames since this slot was last on screen (`None`: never — redraw all).
    pub const fn age(&self) -> Option<u32> {
        self.age
    }

    /// The damage drawn into the slot since it was acquired.
    pub fn damage(&self) -> &[Rect] {
        &self.damage[..self.damage_count]
    }
}

/// Why the ring refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RingError {
    /// Every slot is rendering, queued or scanned.
    NoFreeSlot,
    /// The slot is on screen: not a render target, not flippable.
    Scanned,
    /// The slot is not being rendered into.
    NotRendering,
    /// A flip is already queued; the next vertical blank takes it first.
    FlipPending,
    /// A damage rect leaves the plane.
    DamageOutsidePlane,
    /// More damage than a slot remembers; the caller merges or redraws whole.
    TooMuchDamage,
    /// The pointer's hotspot is outside the display.
    PointerOutsideDisplay,
}

/// The ring of `N` planes of one display.
#[derive(Clone, Debug)]
pub struct PlaneRing<const N: usize> {
    width: u32,
    height: u32,
    slots: [Slot; N],
    scanned: Option<usize>,
    queued: Option<usize>,
    frames: u64,
}

impl<const N: usize> PlaneRing<N> {
    /// A ring of empty planes of `width`x`height`; nothing is scanned yet.
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height, slots: [Slot::EMPTY; N], scanned: None, queued: None, frames: 0 }
    }

    /// The slot on screen, if any.
    pub const fn scanned(&self) -> Option<usize> {
        self.scanned
    }

    /// The slot waiting for the vertical blank, if any.
    pub const fn queued(&self) -> Option<usize> {
        self.queued
    }

    /// Vertical blanks seen.
    pub const fn frames(&self) -> u64 {
        self.frames
    }

    pub fn slot(&self, index: usize) -> Option<&Slot> {
        self.slots.get(index)
    }

    /// A free slot to render into, marked `Rendering`: the one most recently on screen, so
    /// the client repaints the fewest frames of damage; a plane never scanned comes last.
    pub fn acquire(&mut self) -> Result<usize, RingError> {
        let index = self
            .slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.state == SlotState::Free)
            .min_by_key(|(_, s)| s.age.unwrap_or(u32::MAX))
            .map(|(i, _)| i)
            .ok_or(RingError::NoFreeSlot)?;
        self.render_into(index)
    }

    /// Renders into `index` explicitly; the scanned and the queued slot are refused.
    pub fn render_into(&mut self, index: usize) -> Result<usize, RingError> {
        let slot = self.slots.get_mut(index).ok_or(RingError::NoFreeSlot)?;
        match slot.state {
            SlotState::Scanned => Err(RingError::Scanned),
            SlotState::Queued => Err(RingError::FlipPending),
            SlotState::Rendering => Ok(index),
            SlotState::Free => {
                slot.state = SlotState::Rendering;
                slot.damage_count = 0;
                Ok(index)
            }
        }
    }

    /// Gives a rendering slot back without a flip (a frame composed while the splash holds the
    /// glass is never shown); its content is undefined from here on.
    pub fn release(&mut self, index: usize) -> Result<(), RingError> {
        let slot = self.slots.get_mut(index).ok_or(RingError::NotRendering)?;
        if slot.state != SlotState::Rendering {
            return Err(RingError::NotRendering);
        }
        slot.state = SlotState::Free;
        slot.age = None;
        slot.damage_count = 0;
        Ok(())
    }

    /// Records damage drawn into a rendering slot.
    pub fn damage(&mut self, index: usize, rect: Rect) -> Result<(), RingError> {
        let (width, height) = (self.width, self.height);
        let slot = self.slots.get_mut(index).ok_or(RingError::NotRendering)?;
        if slot.state != SlotState::Rendering {
            return Err(RingError::NotRendering);
        }
        let inside = rect.x.checked_add(rect.width).is_some_and(|r| r <= width)
            && rect.y.checked_add(rect.height).is_some_and(|b| b <= height);
        if !inside {
            return Err(RingError::DamageOutsidePlane);
        }
        if slot.damage_count == MAX_DAMAGE {
            return Err(RingError::TooMuchDamage);
        }
        slot.damage[slot.damage_count] = rect;
        slot.damage_count += 1;
        Ok(())
    }

    /// Queues the flip to `index` once `done` is reached; it takes effect at the next
    /// vertical blank.
    pub fn flip(&mut self, index: usize, done: TimelinePoint) -> Result<(), RingError> {
        if self.queued.is_some() {
            return Err(RingError::FlipPending);
        }
        let slot = self.slots.get_mut(index).ok_or(RingError::NotRendering)?;
        match slot.state {
            SlotState::Rendering => {
                slot.state = SlotState::Queued;
                slot.done = done;
                self.queued = Some(index);
                Ok(())
            }
            SlotState::Scanned => Err(RingError::Scanned),
            _ => Err(RingError::NotRendering),
        }
    }

    /// The vertical blank: the queued flip takes effect if its point is reached (the previous
    /// plane becomes free), and every plane's age advances. Returns the slot that came on
    /// screen, if one did.
    pub fn vblank(&mut self, reached: impl Fn(TimelinePoint) -> bool) -> Option<usize> {
        self.frames += 1;
        for slot in self.slots.iter_mut() {
            if let Some(age) = slot.age.as_mut() {
                *age = age.saturating_add(1);
            }
        }
        let index = self.queued?;
        if !reached(self.slots[index].done) {
            return None;
        }
        if let Some(previous) = self.scanned {
            self.slots[previous].state = SlotState::Free;
        }
        self.slots[index].state = SlotState::Scanned;
        self.slots[index].age = Some(0);
        self.slots[index].damage_count = 0;
        self.scanned = Some(index);
        self.queued = None;
        Some(index)
    }
}

/// The vertical blank as the frame clock: when the next one is due.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameClock {
    period_ns: u64,
    last_ns: u64,
    count: u64,
}

impl FrameClock {
    /// A clock of `period_ns` whose first blank is at `now_ns`.
    pub const fn new(period_ns: u64, now_ns: u64) -> Self {
        Self { period_ns, last_ns: now_ns, count: 0 }
    }

    /// A vertical blank happened at `now_ns`.
    pub fn tick(&mut self, now_ns: u64) {
        self.last_ns = now_ns;
        self.count += 1;
    }

    /// When the next blank is due.
    pub const fn next_ns(&self) -> u64 {
        self.last_ns.saturating_add(self.period_ns)
    }

    pub const fn count(&self) -> u64 {
        self.count
    }
}

/// Where the pointer plane shows a sprite: the visible part, whole at every border.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointerPlacement {
    /// The sprite's rows and columns that are on screen.
    pub src_x: u32,
    pub src_y: u32,
    /// Where they land.
    pub dst: Rect,
}

/// The pointer as the controller's own plane: moved without a flip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointerPlane {
    display_w: u32,
    display_h: u32,
    sprite_w: u32,
    sprite_h: u32,
    hot_x: u32,
    hot_y: u32,
}

impl PointerPlane {
    pub const fn new(display: (u32, u32), sprite: (u32, u32), hotspot: (u32, u32)) -> Self {
        Self {
            display_w: display.0,
            display_h: display.1,
            sprite_w: sprite.0,
            sprite_h: sprite.1,
            hot_x: hotspot.0,
            hot_y: hotspot.1,
        }
    }

    /// The placement for a hotspot at (`x`, `y`): clipped at every border, so a sprite that
    /// crosses the right or bottom edge shows its visible part and nothing else.
    pub fn place(&self, x: i32, y: i32) -> Result<PointerPlacement, RingError> {
        if x < 0 || y < 0 || x as u32 >= self.display_w || y as u32 >= self.display_h {
            return Err(RingError::PointerOutsideDisplay);
        }
        let left = i64::from(x) - i64::from(self.hot_x);
        let top = i64::from(y) - i64::from(self.hot_y);
        let src_x = (-left).max(0) as u32;
        let src_y = (-top).max(0) as u32;
        let dst_x = left.max(0) as u32;
        let dst_y = top.max(0) as u32;
        let width = (self.sprite_w - src_x).min(self.display_w - dst_x);
        let height = (self.sprite_h - src_y).min(self.display_h - dst_y);
        Ok(PointerPlacement { src_x, src_y, dst: Rect { x: dst_x, y: dst_y, width, height } })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DONE: TimelinePoint = TimelinePoint(1);
    fn rect(x: u32, y: u32, w: u32, h: u32) -> Rect {
        Rect { x, y, width: w, height: h }
    }
    fn reached(_: TimelinePoint) -> bool {
        true
    }

    #[test]
    fn a_flip_takes_effect_at_the_vertical_blank_not_before() {
        let mut ring = PlaneRing::<3>::new(1920, 1080);
        let a = ring.acquire().unwrap();
        ring.flip(a, DONE).unwrap();
        assert_eq!(ring.scanned(), None, "queued, not scanned");
        assert_eq!(ring.slot(a).unwrap().state(), SlotState::Queued);
        assert_eq!(ring.vblank(|_| false), None, "the drawing is not done: the blank passes");
        assert_eq!(ring.vblank(reached), Some(a));
        assert_eq!(ring.scanned(), Some(a));
        assert_eq!(ring.frames(), 2);
    }

    #[test]
    fn test_reject_render_into_scanned_slot() {
        let mut ring = PlaneRing::<2>::new(8, 8);
        let a = ring.acquire().unwrap();
        ring.flip(a, DONE).unwrap();
        ring.vblank(reached);
        assert_eq!(ring.render_into(a), Err(RingError::Scanned));
        assert_eq!(ring.flip(a, DONE), Err(RingError::Scanned));
        let b = ring.acquire().unwrap();
        assert_ne!(a, b, "acquire never hands out the scanned plane");
        ring.flip(b, DONE).unwrap();
        ring.vblank(reached);
        assert_eq!(ring.slot(a).unwrap().state(), SlotState::Free, "the previous plane is free");
    }

    #[test]
    fn test_reject_flip_of_a_free_slot() {
        let mut ring = PlaneRing::<2>::new(8, 8);
        assert_eq!(ring.flip(0, DONE), Err(RingError::NotRendering));
        assert_eq!(ring.flip(5, DONE), Err(RingError::NotRendering));
        let a = ring.acquire().unwrap();
        ring.flip(a, DONE).unwrap();
        let b = ring.acquire().unwrap();
        assert_eq!(ring.flip(b, DONE), Err(RingError::FlipPending), "one flip at a time");
        assert_eq!(ring.render_into(a), Err(RingError::FlipPending));
    }

    #[test]
    fn a_released_slot_is_free_again_and_knows_nothing() {
        let mut ring = PlaneRing::<2>::new(8, 8);
        let a = ring.acquire().unwrap();
        ring.damage(a, rect(0, 0, 1, 1)).unwrap();
        assert_eq!(ring.release(a), Ok(()));
        assert_eq!(ring.slot(a).unwrap().state(), SlotState::Free);
        assert_eq!(ring.slot(a).unwrap().age(), None);
        assert_eq!(ring.release(a), Err(RingError::NotRendering));
        let b = ring.acquire().unwrap();
        ring.flip(b, DONE).unwrap();
        assert_eq!(ring.release(b), Err(RingError::NotRendering), "queued is not released");
    }

    #[test]
    fn test_reject_damage_outside_the_plane() {
        let mut ring = PlaneRing::<2>::new(1920, 1080);
        let a = ring.acquire().unwrap();
        assert_eq!(ring.damage(a, rect(1900, 0, 21, 10)), Err(RingError::DamageOutsidePlane));
        assert_eq!(ring.damage(a, rect(0, 1080, 1, 1)), Err(RingError::DamageOutsidePlane));
        assert_eq!(ring.damage(a, rect(u32::MAX, 0, 1, 1)), Err(RingError::DamageOutsidePlane));
        assert_eq!(ring.damage(a, rect(1900, 1070, 20, 10)), Ok(()));
        let b = ring.acquire().unwrap();
        ring.flip(b, DONE).unwrap();
        assert_eq!(ring.damage(b, rect(0, 0, 1, 1)), Err(RingError::NotRendering));
        for i in 1..MAX_DAMAGE {
            ring.damage(a, rect(i as u32, 0, 1, 1)).unwrap();
        }
        assert_eq!(ring.damage(a, rect(0, 0, 1, 1)), Err(RingError::TooMuchDamage));
    }

    #[test]
    fn buffer_age_counts_frames_since_the_slot_was_scanned() {
        let mut ring = PlaneRing::<3>::new(8, 8);
        let a = ring.acquire().unwrap();
        assert_eq!(ring.slot(a).unwrap().age(), None, "never on screen: redraw all");
        ring.flip(a, DONE).unwrap();
        ring.vblank(reached);
        assert_eq!(ring.slot(a).unwrap().age(), Some(0));
        let b = ring.acquire().unwrap();
        assert_ne!(a, b);
        ring.flip(b, DONE).unwrap();
        ring.vblank(reached);
        assert_eq!(ring.slot(a).unwrap().age(), Some(1));
        assert_eq!(ring.slot(a).unwrap().state(), SlotState::Free);
        // Two planes suffice while nothing runs ahead: the one scanned a frame ago comes back
        // (one frame of damage to repaint), the never-used third plane stays untouched.
        assert_eq!(ring.acquire(), Ok(a), "the most recently scanned free plane");
        ring.flip(a, DONE).unwrap();
        ring.vblank(reached);
        assert_eq!(ring.slot(b).unwrap().age(), Some(1));
        assert_eq!(ring.slot(a).unwrap().age(), Some(0));
        assert_eq!(ring.acquire(), Ok(b));
        assert_eq!(ring.acquire(), Ok(2), "the third plane only when rendering runs ahead");
        assert_eq!(ring.slot(2).unwrap().age(), None);
        assert_eq!(ring.acquire(), Err(RingError::NoFreeSlot));
    }

    #[test]
    fn the_frame_clock_is_the_vertical_blank() {
        let mut clock = FrameClock::new(16_666_667, 1_000);
        assert_eq!(clock.next_ns(), 16_667_667);
        clock.tick(16_700_000);
        assert_eq!(clock.count(), 1);
        assert_eq!(clock.next_ns(), 33_366_667, "from the blank that happened, not the plan");
    }

    #[test]
    fn the_pointer_plane_moves_without_a_flip_and_is_whole_at_every_border() {
        let pointer = PointerPlane::new((1920, 1080), (64, 64), (2, 2));
        let mid = pointer.place(100, 100).unwrap();
        assert_eq!(mid, PointerPlacement { src_x: 0, src_y: 0, dst: rect(98, 98, 64, 64) });
        // The right border: the sprite's visible columns only (the strip of 2026-10-10).
        let right = pointer.place(1919, 500).unwrap();
        assert_eq!(right.dst, rect(1917, 498, 3, 64));
        assert_eq!((right.src_x, right.src_y), (0, 0));
        // The top-left corner: the hotspot's rows and columns before it are off screen.
        let corner = pointer.place(0, 0).unwrap();
        assert_eq!(corner, PointerPlacement { src_x: 2, src_y: 2, dst: rect(0, 0, 62, 62) });
        let bottom = pointer.place(10, 1079).unwrap();
        assert_eq!(bottom.dst, rect(8, 1077, 64, 3));
    }

    #[test]
    fn test_reject_pointer_rect_outside_the_display() {
        let pointer = PointerPlane::new((1920, 1080), (64, 64), (2, 2));
        assert_eq!(pointer.place(1920, 10), Err(RingError::PointerOutsideDisplay));
        assert_eq!(pointer.place(10, 1080), Err(RingError::PointerOutsideDisplay));
        assert_eq!(pointer.place(-1, 10), Err(RingError::PointerOutsideDisplay));
    }
}
