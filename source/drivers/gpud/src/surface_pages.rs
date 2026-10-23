// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the surface object's pages the GPU has been told (ADR-0078). The object — the
//! wallpaper, the retained plane and the atlas — is bound into windowd's GPU context as one sparse
//! range with nothing entered; before a frame's first pass gpud enters the pages of every
//! rectangle the frame samples that are not entered yet: the plane the scene starts from and each
//! layer's band. Every texture a frame draws is a piece of one of those (`nexus_gfx::frame`), and
//! a piece's texels lie in its rows — so entering whole rows covers every texel a descriptor
//! reads (address `row × stride + x × 4`, `h` rows of `w × 4` bytes), which is what the tests
//! below prove. A row of the 1080p layout is 1.875 pages; entered whole, a sampled band costs its
//! rows, the measure the atlas was sized by (`windowd: atlas rows`).
//! OWNERS: @gpu @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests below — every texel inside its rows' pages, a page entered once, in runs,
//!   nothing past the object (`test_reject_entering_past_the_object`)

use alloc::vec;
use alloc::vec::Vec;

/// One page of the GPU's MMU.
pub(crate) const PAGE: u64 = 4096;

/// The pages rows `[row, row + h)` of an object of `stride` bytes a row span: `[first, end)`
/// (empty for no rows).
pub(crate) fn row_pages(stride: u64, row: u32, h: u32) -> (u64, u64) {
    if h == 0 {
        return (0, 0);
    }
    let start = u64::from(row) * stride;
    let end = (u64::from(row) + u64::from(h)) * stride;
    (start / PAGE, end.div_ceil(PAGE))
}

/// The pages entered in the GPU's tables, one bit each.
pub(crate) struct Entered {
    words: Vec<u64>,
    pages: u64,
    count: u64,
}

impl Entered {
    /// An object of `pages` pages, none entered.
    pub(crate) fn new(pages: u64) -> Self {
        Self { words: vec![0; pages.div_ceil(64) as usize], pages, count: 0 }
    }

    fn bit(&self, page: u64) -> bool {
        (self.words[(page / 64) as usize] >> (page % 64)) & 1 == 1
    }

    /// Whether every page of `[first, end)` is entered — a word at a time where it can.
    pub(crate) fn all(&self, first: u64, end: u64) -> bool {
        if end > self.pages {
            return false;
        }
        let mut p = first;
        while p < end {
            if p % 64 == 0 && end - p >= 64 {
                if self.words[(p / 64) as usize] != u64::MAX {
                    return false;
                }
                p += 64;
            } else {
                if !self.bit(p) {
                    return false;
                }
                p += 1;
            }
        }
        true
    }

    /// Calls `enter` with each run `[a, b)` of `[first, end)` that is not entered yet, in order,
    /// and marks it entered when `enter` answers `true`. `false` — nothing past the object is
    /// entered, and a refused run stops the walk with itself and the rest unmarked.
    pub(crate) fn enter_missing(
        &mut self,
        first: u64,
        end: u64,
        mut enter: impl FnMut(u64, u64) -> bool,
    ) -> bool {
        if end > self.pages || first > end {
            return false;
        }
        let mut p = first;
        while p < end {
            if self.bit(p) {
                p += 1;
                continue;
            }
            let run = p;
            while p < end && !self.bit(p) {
                p += 1;
            }
            if !enter(run, p) {
                return false;
            }
            for page in run..p {
                self.words[(page / 64) as usize] |= 1 << (page % 64);
            }
            self.count += p - run;
        }
        true
    }

    /// Pages entered.
    pub(crate) const fn count(&self) -> u64 {
        self.count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STRIDE: u64 = 1920 * 4;

    #[test]
    fn rows_span_the_pages_their_bytes_lie_in() {
        assert_eq!(row_pages(STRIDE, 0, 1), (0, 2), "bytes 0..7680");
        assert_eq!(row_pages(STRIDE, 1, 1), (1, 4), "bytes 7680..15360");
        assert_eq!(row_pages(STRIDE, 0, 1080), (0, 2025), "a plane is 2025 pages");
        assert_eq!(row_pages(STRIDE, 5, 0), (0, 0));
    }

    /// The pages entered for a rectangle's rows hold every texel a descriptor over it reads:
    /// address `row × stride + x × 4`, `h` rows of `w × 4` bytes — for rectangles at every
    /// column, width and row phase against the pages.
    #[test]
    fn every_texel_a_descriptor_reads_lies_in_an_entered_page() {
        let rects: [(u32, u32, u32, u32); 5] = [
            (0, 0, 1920, 1080),
            (1919, 7, 1, 3),
            (360, 5400, 1080, 853),
            (1500, 8639, 420, 1),
            (17, 3, 1, 1),
        ];
        for (x, row, w, h) in rects {
            let (first, end) = row_pages(STRIDE, row, h);
            for r in row..row + h {
                let lo = u64::from(r) * STRIDE + u64::from(x) * 4;
                let hi = lo + u64::from(w) * 4 - 1;
                assert!(lo / PAGE >= first && hi / PAGE < end, "({x},{row},{w},{h}) row {r}");
            }
        }
    }

    /// A page is entered once: the missing ones come in runs, in order; asked again, nothing is
    /// missing and nothing is entered twice.
    #[test]
    fn a_page_is_entered_once_in_runs() {
        let mut e = Entered::new(200);
        let mut runs = Vec::new();
        assert!(e.enter_missing(10, 20, |a, b| {
            runs.push((a, b));
            true
        }));
        assert!(e.enter_missing(0, 130, |a, b| {
            runs.push((a, b));
            true
        }));
        assert_eq!(runs, [(10, 20), (0, 10), (20, 130)]);
        assert_eq!(e.count(), 130);
        assert!(e.all(0, 130) && !e.all(0, 131) && e.all(64, 128));
        assert!(e.enter_missing(5, 125, |_, _| panic!("nothing is missing")));
    }

    /// A run the device refused stays unentered, and so does everything after it.
    #[test]
    fn test_reject_marking_a_refused_run() {
        let mut e = Entered::new(64);
        assert!(e.enter_missing(0, 4, |_, _| true));
        assert!(!e.enter_missing(0, 32, |a, _| a != 4));
        assert_eq!(e.count(), 4);
        assert!(!e.all(4, 5));
    }

    /// Nothing past the object is entered or answered as entered.
    #[test]
    fn test_reject_entering_past_the_object() {
        let mut e = Entered::new(100);
        assert!(!e.enter_missing(90, 101, |_, _| panic!("past the object")));
        assert!(!e.enter_missing(50, 40, |_, _| panic!("a reversed range")));
        assert!(!e.all(0, 101));
        assert_eq!(e.count(), 0);
    }
}
