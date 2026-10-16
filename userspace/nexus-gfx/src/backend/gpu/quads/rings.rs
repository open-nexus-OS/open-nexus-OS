// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A context's pass memory as rings (TASK-0215 5c-3 P1): the four regions a pass writes — the
//! general arena, the PDS heap, the stream buffer, the vertex buffer — each a ring of 64-byte
//! lines. A pass's blocks are bumped from the ring's head (wrapping to its start); its lines stay
//! taken until the timeline has reached the point the pass was submitted at, then the next
//! passes write them (the open driver's transient memory, reused once its job is done). Nothing
//! is allocated: a fixed queue of passes in flight, a fixed four rings.

use super::{Need, Placer, DESCRIPTOR_BYTES};

/// Passes a ring set keeps in flight.
pub const MAX_IN_FLIGHT: usize = 16;
/// A line: every block starts on one.
const LINE: u32 = 64;

/// A region's ring: `len` bytes from `base` (offsets in the region's own terms — the general
/// arena's bytes, PDS-heap offsets, the stream and vertex buffers' bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Arena {
    pub base: u32,
    pub len: u32,
}

#[derive(Clone, Copy, Debug, Default)]
struct Ring {
    base: u32,
    len: u32,
    /// The next free byte, from `base`.
    head: u32,
    /// The oldest taken byte, from `base`.
    tail: u32,
    /// Bytes taken (in flight and the open pass), the bytes skipped at a wrap included.
    used: u32,
}

impl Ring {
    fn new(a: Arena) -> Self {
        let len = a.len - a.len % LINE;
        Self { base: a.base, len, head: 0, tail: 0, used: 0 }
    }

    /// `bytes` (whole lines) at the head, wrapping when the run to the end is short; one line
    /// always stays free, so a full ring and an empty one never look alike.
    fn take(&mut self, bytes: u32) -> Option<u32> {
        let bytes = bytes.checked_next_multiple_of(LINE)?;
        let free = self.len - self.used;
        let to_end = self.len - self.head;
        let (at, skip) = if bytes <= to_end { (self.head, 0) } else { (0, to_end) };
        let need = bytes.checked_add(skip)?;
        if need >= free {
            return None;
        }
        self.used += need;
        self.head = (at + bytes) % self.len;
        Some(self.base + at)
    }

    /// The bytes from the tail to `end`, freed.
    fn free_to(&mut self, end: u32) {
        let span = (end + self.len - self.tail) % self.len;
        self.used -= span.min(self.used);
        self.tail = end;
    }
}

/// Why a pass could not be put in flight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RingError {
    /// [`MAX_IN_FLIGHT`] passes are already in flight: retire first.
    Full,
}

/// The four rings, the passes in flight and the open pass.
#[derive(Clone, Copy, Debug)]
pub struct Rings {
    rings: [Ring; 4],
    /// Passes in flight, oldest first: their point and each ring's head after them.
    flight: [(u64, [u32; 4]); MAX_IN_FLIGHT],
    first: usize,
    count: usize,
    /// The open pass's start (each ring's head when it began).
    open: [u32; 4],
}

impl Rings {
    pub fn new(general: Arena, pds: Arena, stream: Arena, vertex: Arena) -> Self {
        let rings = [Ring::new(general), Ring::new(pds), Ring::new(stream), Ring::new(vertex)];
        Self { rings, flight: [(0, [0; 4]); MAX_IN_FLIGHT], first: 0, count: 0, open: [0; 4] }
    }

    /// A pass begins: its blocks follow. An empty ring starts again from its first line — a
    /// block never wraps, so a head left near the end would refuse a block the empty ring holds
    /// (board cycle 21: five vertex lines after a pass that ended five lines in, of seven).
    pub fn begin(&mut self) {
        for r in self.rings.iter_mut().filter(|r| r.used == 0) {
            (r.head, r.tail) = (0, 0);
        }
        self.open = self.rings.map(|r| r.head);
    }

    /// The open pass was submitted at `point`: its lines are taken until the point is reached.
    pub fn submitted(&mut self, point: u64) -> Result<(), RingError> {
        if self.count == MAX_IN_FLIGHT {
            return Err(RingError::Full);
        }
        let at = (self.first + self.count) % MAX_IN_FLIGHT;
        self.flight[at] = (point, self.rings.map(|r| r.head));
        self.count += 1;
        self.begin();
        Ok(())
    }

    /// The open pass was not submitted: its lines are free again.
    pub fn abandon(&mut self) {
        for (r, start) in self.rings.iter_mut().zip(self.open) {
            let span = (r.head + r.len - start) % r.len;
            r.used -= span.min(r.used);
            r.head = start;
        }
    }

    /// Frees the lines of every pass in flight whose point `reached` says the timeline passed,
    /// oldest first (a pass is reached only after every pass before it).
    pub fn retire(&mut self, reached: &dyn Fn(u64) -> bool) {
        while self.count > 0 {
            let (point, ends) = self.flight[self.first];
            if !reached(point) {
                break;
            }
            for (r, end) in self.rings.iter_mut().zip(ends) {
                r.free_to(end);
            }
            self.first = (self.first + 1) % MAX_IN_FLIGHT;
            self.count -= 1;
        }
    }

    /// Passes in flight.
    pub fn in_flight(&self) -> usize {
        self.count
    }

    /// The oldest pass in flight's point.
    pub fn oldest(&self) -> Option<u64> {
        (self.count > 0).then(|| self.flight[self.first].0)
    }
}

impl Placer for Rings {
    fn place(&mut self, _draw: usize, need: Need) -> Option<u32> {
        let (ring, bytes) = match need {
            Need::Push(n) => (0, n),
            Need::Ppp => (0, LINE),
            Need::Ppp26 => (0, 2 * LINE),
            Need::Descriptor => (0, DESCRIPTOR_BYTES),
            Need::StateData(n) => (1, n),
            Need::VsPdsData => (1, LINE),
            Need::Stream(n) => (2, n),
            Need::Vertices(n) => (3, n),
        };
        self.rings[ring].take(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rings(len: u32) -> Rings {
        let a = |base| Arena { base, len };
        Rings::new(a(0x1_0000), a(0x3000), a(0x800), a(0x800))
    }

    #[test]
    fn a_pass_takes_lines_from_each_ring_in_turn() {
        let mut r = rings(0x400);
        r.begin();
        assert_eq!(r.place(0, Need::Stream(60)), Some(0x800));
        assert_eq!(r.place(0, Need::Vertices(96)), Some(0x800));
        assert_eq!(r.place(0, Need::VsPdsData), Some(0x3000));
        assert_eq!(r.place(0, Need::Push(64)), Some(0x1_0000));
        assert_eq!(r.place(0, Need::Ppp26), Some(0x1_0040));
        assert_eq!(r.place(1, Need::Ppp), Some(0x1_00c0));
    }

    #[test]
    fn lines_return_only_when_their_pass_is_reached() {
        // A ring of 16 lines: three passes of five lines each, one line always free.
        let mut r = rings(16 * LINE);
        for point in 1..=3 {
            r.begin();
            assert!(r.place(0, Need::Push(5 * LINE)).is_some(), "pass {point}");
            r.submitted(point).unwrap();
        }
        r.begin();
        assert_eq!(r.place(0, Need::Push(LINE)), None, "fifteen lines in flight");
        r.abandon();
        // Pass 1 reached: its five lines return; the next pass skips the last line and wraps
        // into them (four lines and the skipped one: five of the six free, one kept free).
        r.retire(&|p| p <= 1);
        assert_eq!(r.in_flight(), 2);
        r.begin();
        assert_eq!(r.place(0, Need::Push(5 * LINE)), None, "five more would fill the ring");
        assert_eq!(r.place(0, Need::Push(4 * LINE)), Some(0x1_0000), "wrapped to the start");
        r.submitted(4).unwrap();
        assert_eq!(r.oldest(), Some(2));
    }

    #[test]
    fn a_retire_stops_at_the_first_pass_not_reached() {
        let mut r = rings(16 * LINE);
        for point in 1..=3 {
            r.begin();
            r.place(0, Need::Push(LINE)).unwrap();
            r.submitted(point).unwrap();
        }
        r.retire(&|p| p == 1 || p == 3);
        assert_eq!(r.oldest(), Some(2), "pass 3 waits behind pass 2");
    }

    #[test]
    fn an_emptied_ring_holds_a_block_its_tail_would_not() {
        // Seven usable lines; a pass of five ends five lines in. Once it is reached, the next
        // pass's five lines start again at the first line instead of failing at the end.
        let mut r = rings(8 * LINE);
        r.begin();
        r.place(0, Need::Vertices(5 * LINE)).unwrap();
        r.submitted(1).unwrap();
        r.retire(&|_| true);
        r.begin();
        assert_eq!(r.place(0, Need::Vertices(5 * LINE)), Some(0x800));
    }

    #[test]
    fn an_abandoned_pass_gives_its_lines_back() {
        let mut r = rings(16 * LINE);
        r.begin();
        r.place(0, Need::Push(10 * LINE)).unwrap();
        r.abandon();
        r.begin();
        assert_eq!(r.place(0, Need::Push(15 * LINE)), Some(0x1_0000));
    }

    #[test]
    fn test_reject_more_passes_in_flight_than_the_queue() {
        let mut r = rings(0x10_0000);
        for point in 0..MAX_IN_FLIGHT as u64 {
            r.begin();
            r.place(0, Need::Push(LINE)).unwrap();
            r.submitted(point).unwrap();
        }
        r.begin();
        r.place(0, Need::Push(LINE)).unwrap();
        assert_eq!(r.submitted(99), Err(RingError::Full));
    }

    #[test]
    fn many_passes_reuse_the_same_lines_without_growth() {
        // A frame's worth of passes, each retired as the next begins: the ring wraps many times
        // and every block stays inside it.
        let mut r = rings(8 * LINE);
        for point in 0..1000u64 {
            r.retire(&|p| p < point);
            r.begin();
            let at = r.place(0, Need::Push(3 * LINE)).expect("room after the retire");
            assert!((0x1_0000..0x1_0000 + 8 * LINE).contains(&at));
            r.submitted(point).unwrap();
        }
    }
}
