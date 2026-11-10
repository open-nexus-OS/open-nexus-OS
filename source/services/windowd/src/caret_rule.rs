// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the compositor caret's two rules (RFC-0113 §5, ADR-0090), pure: when it shows — a
//! blink of `BLINK_HALF_NS` on and off from its last reset (an edit, a move: solid first), resting
//! solid after `BLINK_FOR_NS` without one, so a caret nobody types at costs no wake — and where:
//! its rectangle in surface pixels placed at the surface's origin, clipped to the surface and the
//! screen; a caret outside its surface is refused, never drawn over a neighbour.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below (the phase, the rest, the clip, `test_reject_*`)

use nexus_display_proto::surface_caret::{Caret, CARET_BLINK};

/// Half a blink: on this long, then off this long.
pub const BLINK_HALF_NS: u64 = 530_000_000;
/// The blink rests solid this long after the last reset.
pub const BLINK_FOR_NS: u64 = 10_000_000_000;

/// Whether the caret shows at `now_ns` (its blink restarted at `reset_ns`), and when that changes
/// next — `None`: it rests (a steady caret, or a blink past `BLINK_FOR_NS`), no wake owed.
#[must_use]
pub fn phase(reset_ns: u64, now_ns: u64, caret: &Caret) -> (bool, Option<u64>) {
    if caret.flags & CARET_BLINK == 0 {
        return (true, None);
    }
    let elapsed = now_ns.saturating_sub(reset_ns);
    let rest_at = reset_ns.saturating_add(BLINK_FOR_NS);
    if elapsed >= BLINK_FOR_NS {
        return (true, None);
    }
    let half = elapsed / BLINK_HALF_NS;
    let next = reset_ns.saturating_add((half + 1) * BLINK_HALF_NS).min(rest_at);
    (half % 2 == 0, Some(next))
}

/// The caret's screen rectangle `(x, y, w, h)`: its surface rectangle at the surface's `origin`,
/// clipped to the surface (`surface` w × h) and the screen (`mode`); `None` when nothing of it is
/// inside its surface.
#[must_use]
pub fn screen_rect(
    origin: (i32, i32),
    surface: (u32, u32),
    caret: &Caret,
    mode: (u32, u32),
) -> Option<(u32, u32, u32, u32)> {
    let (cx, cy) = (u32::from(caret.x), u32::from(caret.y));
    let (cw, ch) = (u32::from(caret.w), u32::from(caret.h));
    // Inside the surface first: a caret is the surface's own, never a neighbour's pixels.
    let w = cw.min(surface.0.saturating_sub(cx));
    let h = ch.min(surface.1.saturating_sub(cy));
    if cx >= surface.0 || cy >= surface.1 || w == 0 || h == 0 {
        return None;
    }
    // Then on the screen.
    let (x0, y0) = (origin.0 + cx as i32, origin.1 + cy as i32);
    let (x1, y1) = (x0 + w as i32, y0 + h as i32);
    let (sx0, sy0) = (x0.max(0), y0.max(0));
    let (sx1, sy1) = (x1.min(mode.0 as i32), y1.min(mode.1 as i32));
    (sx1 > sx0 && sy1 > sy0)
        .then(|| (sx0 as u32, sy0 as u32, (sx1 - sx0) as u32, (sy1 - sy0) as u32))
}

/// How many surfaces keep their declared caret at once — one per window that holds a focused
/// field; past that the oldest declaration yields (its app re-declares on its next change).
pub const CARET_SURFACES: usize = 8;

/// Every surface's last declared caret (RFC-0113 §5). An app declares its caret only when it
/// changes, so ONE held caret lost a window's caret to the next window's: focus back on the first
/// field, the same caret as before — nothing re-declared, nothing shown (board cycle 2026-11-09:
/// no caret in a chat field after the shell's clipboard search). The table keeps each surface's,
/// and the compositor shows the focused one's.
#[derive(Clone, Copy, Debug)]
pub struct CaretTable {
    entries: [Option<(u32, Caret)>; CARET_SURFACES],
    /// When each entry was last declared (a running count), so the oldest yields.
    stamps: [u64; CARET_SURFACES],
    next: u64,
}

impl Default for CaretTable {
    fn default() -> Self {
        Self::new()
    }
}

impl CaretTable {
    pub const fn new() -> Self {
        Self { entries: [None; CARET_SURFACES], stamps: [0; CARET_SURFACES], next: 0 }
    }

    /// `surface` declares `caret`. `true` when it is new or its rectangle moved — the blink
    /// restarts solid.
    pub fn declare(&mut self, surface: u32, caret: Caret) -> bool {
        let rect = |c: &Caret| (c.x, c.y, c.w, c.h);
        self.next += 1;
        let at = self.entries.iter().position(|e| e.is_some_and(|(s, _)| s == surface));
        let (slot, moved) = match at {
            Some(i) => (i, self.entries[i].is_some_and(|(_, c)| rect(&c) != rect(&caret))),
            None => {
                let free = self.entries.iter().position(Option::is_none);
                let oldest = (0..CARET_SURFACES).min_by_key(|&i| self.stamps[i]).unwrap_or(0);
                (free.unwrap_or(oldest), true)
            }
        };
        self.entries[slot] = Some((surface, caret));
        self.stamps[slot] = self.next;
        moved
    }

    /// `surface` hides its caret (no field focused, a selection showing) or is gone.
    pub fn hide(&mut self, surface: u32) {
        for entry in &mut self.entries {
            if entry.is_some_and(|(s, _)| s == surface) {
                *entry = None;
            }
        }
    }

    /// `surface`'s caret, if it declared one.
    pub fn get(&self, surface: u32) -> Option<Caret> {
        self.entries.iter().flatten().find(|(s, _)| *s == surface).map(|(_, c)| *c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_display_proto::surface_caret::CARET_VISIBLE;

    const MS: u64 = 1_000_000;

    fn caret(flags: u8) -> Caret {
        Caret { x: 10, y: 4, w: 2, h: 20, bgra: [0, 0, 0, 255], flags: CARET_VISIBLE | flags }
    }

    #[test]
    fn a_blink_starts_solid_alternates_and_rests() {
        let c = caret(CARET_BLINK);
        assert_eq!(phase(1_000, 1_000, &c), (true, Some(1_000 + BLINK_HALF_NS)), "solid first");
        assert_eq!(phase(0, BLINK_HALF_NS + MS, &c).0, false, "then off");
        assert_eq!(phase(0, 2 * BLINK_HALF_NS + MS, &c).0, true, "then on again");
        assert_eq!(phase(0, BLINK_FOR_NS, &c), (true, None), "rests solid, no wake");
        let (_, last) = phase(0, BLINK_FOR_NS - MS, &c);
        assert_eq!(last, Some(BLINK_FOR_NS), "the last change is the rest itself");
        assert_eq!(phase(0, 5 * MS, &caret(0)), (true, None), "a steady caret never wakes");
    }

    #[test]
    fn the_caret_lands_at_its_surface_origin_clipped_to_the_screen() {
        let c = caret(0);
        assert_eq!(screen_rect((100, 50), (400, 300), &c, (1280, 800)), Some((110, 54, 2, 20)));
        // A window half off the left edge: what is on screen is drawn.
        let edge = Caret { x: 2, ..c };
        assert_eq!(screen_rect((-3, 0), (400, 300), &edge, (1280, 800)), Some((0, 4, 1, 20)));
    }

    #[test]
    fn test_reject_caret_outside_its_surface() {
        let c = caret(0);
        assert_eq!(screen_rect((0, 0), (8, 300), &c, (1280, 800)), None, "right of its surface");
        assert_eq!(screen_rect((0, 0), (400, 4), &c, (1280, 800)), None, "below its surface");
        let tall = Caret { y: 290, ..c };
        assert_eq!(
            screen_rect((0, 0), (400, 300), &tall, (1280, 800)),
            Some((10, 290, 2, 10)),
            "cut at the surface's edge, never into a neighbour"
        );
        assert_eq!(screen_rect((2000, 0), (400, 300), &c, (1280, 800)), None, "off screen");
    }

    #[test]
    fn a_surfaces_caret_survives_another_surfaces_caret() {
        let mut t = CaretTable::new();
        let chat = caret(CARET_BLINK);
        let search = Caret { x: 300, ..chat };
        assert!(t.declare(7, chat), "new: the blink starts solid");
        assert!(t.declare(3, search), "another window's field");
        assert_eq!(t.get(7), Some(chat), "the first window's caret is still there for its focus");
        assert!(!t.declare(7, chat), "the same caret again is no move");
        t.hide(3);
        assert_eq!((t.get(3), t.get(7)), (None, Some(chat)), "a hide takes only its own caret");
    }

    #[test]
    fn test_reject_a_table_that_grows_past_its_bound() {
        let mut t = CaretTable::new();
        for surface in 0..CARET_SURFACES as u32 {
            t.declare(surface, caret(0));
        }
        t.declare(0, Caret { x: 11, ..caret(0) }); // surface 0 is now the freshest
        t.declare(100, caret(0)); // full: the oldest declaration (surface 1) yields
        assert_eq!(t.get(1), None, "the oldest yielded");
        assert!(t.get(0).is_some() && t.get(100).is_some(), "the fresh ones stay");
        assert_eq!(t.entries.iter().flatten().count(), CARET_SURFACES, "never more than the bound");
    }
}
