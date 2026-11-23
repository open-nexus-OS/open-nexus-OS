// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: A frame's glass writes in waves (TASK-0351 P19). A write is three steps — the
//! scene below it into the scratch at the blur's resolution, the blur across into the half
//! scratch, the blur down into its cache rows — and a pass costs the board's GPU half a
//! millisecond whatever it draws. The writes of a wave share the scratches: each draws its
//! scene in its own pass at its own rows of the scratch (its own scissor — a draw that cannot
//! be cut is clipped by it), then ONE pass blurs every scene across into its own half rows,
//! then each blurs down into its cache rows. A wave is a run of the frame's writes in their
//! order; a write starts the next wave when
//! - its scene shows a glass an earlier write of the wave blurs (that glass's rows are fresh
//!   only after its own blur down, which comes after every scene of the wave),
//! - the scratch or the half scratch has no rows left for it, or
//! - the wave holds `MERGE_MAX` writes.
//!
//! The blur down is a pass a write, not one a wave: a pass has one rectangle, the GPU clears
//! what no draw covers inside it in every tile a draw touches, and the cache plane holds what
//! must stay — a band write's other texels, the rows of the glass allocated next to it. (The
//! half scratch holds nothing a wave does not draw, so the pass across may span them all.)
//! Passes a frame: `2 · writes + waves + 1`, where one by one took `3 · writes + 1`.
//! OWNERS: @gpu @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below (the rule on a stated geometry); `tests/frame_waves.rs`
//!   (the board's passes run on the host: waves leave the rows one by one leaves)
//! INVARIANTS: pure, allocation-free, a fixed table (the board lies on its executor's stack);
//!   a write past the table runs alone — more passes, never a refused frame; a write that
//!   fits nowhere still has its place (the top of a wave of its own).

/// The most glass writes of one wave.
pub const MERGE_MAX: usize = 32;

/// The writes a frame lays out in waves: two waves at their fullest. A write past them runs
/// alone.
pub(super) const PLACED: usize = 2 * MERGE_MAX;

/// What a write takes at a place: rows of the scratch and of the half scratch, and whether its
/// scene shows a glass an earlier write of the wave blurs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Fit {
    pub rows: (u32, u32),
    pub shows: bool,
}

/// Where a write's scene and its horizontal result lie, and in which wave.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Place {
    wave: u16,
    scene_row: u16,
    half_row: u16,
}

/// A pass of a wave: the write it belongs to, or the wave's writes it spans.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    /// Write `w`'s scene.
    Scene(usize),
    /// The blur across of the `count` writes from `from`.
    Across { from: usize, count: usize },
    /// Write `w`'s blur down.
    Down(usize),
}

/// The frame's writes laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Waves {
    places: [Place; PLACED],
    writes: usize,
    /// Waves of the writes in the table.
    placed_waves: usize,
}

impl Waves {
    /// `writes` writes laid out in their order. `fit(w, first, at)` is what write `w` takes at
    /// the scene row and half row `at` in a wave whose first write is `first` — `None` when
    /// the scratches cannot hold it there.
    pub(super) fn lay_out(
        writes: usize,
        fit: impl Fn(usize, usize, (u32, u32)) -> Option<Fit>,
    ) -> Self {
        let mut s = Self { places: [Place::default(); PLACED], writes, placed_waves: 0 };
        let (mut wave, mut first, mut at) = (0u16, 0usize, (0u32, 0u32));
        for w in 0..writes.min(PLACED) {
            let joins = (w > first && w - first < MERGE_MAX)
                .then(|| fit(w, first, at))
                .flatten()
                .filter(|f| !f.shows)
                .zip(Place::at(wave, at));
            let (taken, place) = match joins {
                Some((f, place)) => (Some(f), place),
                None => {
                    wave += u16::from(w > 0);
                    (first, at) = (w, (0, 0));
                    (fit(w, w, at), Place { wave, scene_row: 0, half_row: 0 })
                }
            };
            s.places[w] = place;
            if let Some(f) = taken {
                at = (at.0.saturating_add(f.rows.0), at.1.saturating_add(f.rows.1));
            }
            s.placed_waves = usize::from(wave) + 1;
        }
        s
    }

    /// Waves of the frame.
    pub(super) fn count(&self) -> usize {
        self.placed_waves + self.writes.saturating_sub(PLACED)
    }

    /// Passes of the frame's writes: a scene and a blur down a write, a blur across a wave.
    pub(super) fn passes(&self) -> usize {
        2 * self.writes + self.count()
    }

    /// The scene row and the half row of write `w`.
    pub(super) fn rows(&self, w: usize) -> (u32, u32) {
        self.places.get(w).map_or((0, 0), |p| (u32::from(p.scene_row), u32::from(p.half_row)))
    }

    /// The wave of write `w`.
    fn wave(&self, w: usize) -> usize {
        self.places.get(w).map_or(self.placed_waves + w.saturating_sub(PLACED), |p| p.wave.into())
    }

    /// Pass `p` of the writes' passes: wave after wave, in a wave its scenes, its blur across,
    /// its blurs down.
    pub(super) fn step(&self, mut p: usize) -> Option<Step> {
        let mut from = 0;
        while from < self.writes {
            let wave = self.wave(from);
            let count = (from..self.writes).take_while(|w| self.wave(*w) == wave).count();
            if p < count {
                return Some(Step::Scene(from + p));
            }
            if p == count {
                return Some(Step::Across { from, count });
            }
            if p <= 2 * count {
                return Some(Step::Down(from + p - count - 1));
            }
            p -= 2 * count + 1;
            from += count;
        }
        None
    }
}

impl Place {
    /// The place at rows `at` of `wave`; `None` when a row is past what the table holds (no
    /// room: the write starts a wave).
    fn at(wave: u16, at: (u32, u32)) -> Option<Self> {
        Some(Self {
            wave,
            scene_row: u16::try_from(at.0).ok()?,
            half_row: u16::try_from(at.1).ok()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every write takes 100 scene rows and 150 half rows of scratches 1080 and 600 rows high;
    /// `shows(w, first)` says whose scene shows a glass of its wave.
    fn laid(writes: usize, shows: impl Fn(usize, usize) -> bool) -> Waves {
        Waves::lay_out(writes, |w, first, at| {
            (at.0 + 100 <= 1080 && at.1 + 150 <= 600)
                .then(|| Fit { rows: (100, 150), shows: shows(w, first) })
        })
    }

    fn steps(w: &Waves) -> Vec<Step> {
        (0..w.passes()).map(|p| w.step(p).expect("a pass")).collect()
    }

    #[test]
    fn writes_apart_share_a_wave_and_its_blur_across() {
        let w = laid(3, |_, _| false);
        assert_eq!((w.count(), w.passes()), (1, 7));
        assert_eq!([w.rows(0), w.rows(1), w.rows(2)], [(0, 0), (100, 150), (200, 300)]);
        use Step::{Across, Down, Scene};
        assert_eq!(
            steps(&w),
            [Scene(0), Scene(1), Scene(2), Across { from: 0, count: 3 }, Down(0), Down(1), Down(2)]
        );
        assert_eq!(w.step(7), None);
        assert_eq!(laid(0, |_, _| false).passes(), 0);
    }

    #[test]
    fn test_reject_a_write_sharing_a_wave_with_the_glass_it_blurs() {
        // Write 1's scene shows write 0's glass: its scene pass would run before write 0's
        // blur down and read last frame's rows. It starts a wave; write 2 joins it.
        let w = laid(3, |w, first| w == 1 && first == 0);
        assert_eq!((w.count(), w.passes()), (2, 8));
        assert_eq!([w.rows(0), w.rows(1), w.rows(2)], [(0, 0), (0, 0), (100, 150)]);
        use Step::{Across, Down, Scene};
        assert_eq!(
            steps(&w),
            [
                Scene(0),
                Across { from: 0, count: 1 },
                Down(0),
                Scene(1),
                Scene(2),
                Across { from: 1, count: 2 },
                Down(1),
                Down(2)
            ]
        );
    }

    #[test]
    fn test_reject_a_wave_past_the_scratch() {
        // Four writes of 150 half rows fill the half scratch's 600: the fifth starts a wave at
        // the top — no place of a wave lies past a scratch.
        let w = laid(9, |_, _| false);
        assert_eq!(w.count(), 3);
        for k in 0..9 {
            let (scene, half) = w.rows(k);
            assert_eq!((scene, half), ((k as u32 % 4) * 100, (k as u32 % 4) * 150), "write {k}");
            assert!(scene + 100 <= 1080 && half + 150 <= 600);
        }
        assert_eq!(w.passes(), 2 * 9 + 3);
    }

    #[test]
    fn test_reject_more_writes_in_a_wave_than_its_bound() {
        // Writes that take a row each: a wave ends at `MERGE_MAX`, and a write past the table
        // runs alone.
        let w = Waves::lay_out(PLACED + 3, |_, _, _| Some(Fit { rows: (1, 1), shows: false }));
        assert_eq!(w.count(), 2 + 3);
        assert_eq!(w.step(0), Some(Step::Scene(0)));
        assert_eq!(w.step(MERGE_MAX), Some(Step::Across { from: 0, count: MERGE_MAX }));
        assert_eq!(w.rows(MERGE_MAX), (0, 0), "the next wave starts at the top");
        let alone = 2 * (2 * MERGE_MAX + 1);
        assert_eq!(w.step(alone), Some(Step::Scene(PLACED)));
        assert_eq!(w.step(alone + 1), Some(Step::Across { from: PLACED, count: 1 }));
        assert_eq!(w.step(alone + 2), Some(Step::Down(PLACED)));
        assert_eq!((w.rows(PLACED + 2), w.passes()), ((0, 0), 2 * (PLACED + 3) + 5));
    }

    #[test]
    fn a_write_that_fits_nowhere_keeps_a_wave_of_its_own() {
        // Write 1 fits no scratch: it has its place at the top of its own wave (its passes
        // are the board's to refuse), and the writes around it are laid out as ever.
        let w =
            Waves::lay_out(3, |w, _, _| (w != 1).then_some(Fit { rows: (10, 10), shows: false }));
        assert_eq!([w.rows(0), w.rows(1), w.rows(2)], [(0, 0), (0, 0), (0, 0)]);
        assert_eq!(w.count(), 2, "write 2 joins the wave write 1 opened");
    }
}
