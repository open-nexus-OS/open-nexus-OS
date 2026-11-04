// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The effect kernels (ADR-0094, TASK-0351 P7): an effect's mathematics defined once,
//! as data, read by every executor. The glass blur is the separable gaussian at σ = R/2 with
//! taps out to ±R full-resolution pixels and weights 2^(k·d²), normalised — [`Gaussian`] gives
//! the weights at the library's reduced resolution (a tap every `scale` pixels: the shader
//! pipelines' override constants, emitted by `nxgpu` from this table) and at full resolution
//! (the raster, the oracle every executor's picture is judged by: `raster::blur_gaussian`,
//! `frame::oracle`). The library's radii and the resolution each runs at are the table's too.
//! No tap weight, radius or sigma rule lives in a shader source, a manifest or a device server.
//! OWNERS: @gpu @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below (the weights sum to one, the reduced weights are the full
//!   ones at their distances, the taps a pass fetches); `tools/nxgpu/tests/blur.rs` (the
//!   shader run on the host against the full-resolution kernel); `tests/frame_oracle.rs`
//! INVARIANTS: pure, allocation-free; a library radius's reduced kernel has at most `MAX_TAPS`
//!   taps a side (the shader's override count), a full one at most `MAX_FULL_TAPS`.

/// The radii the library blurs at (windowd's glass levels).
pub const BLUR_RADII: [u32; 4] = [8, 20, 30, 40];
/// Taps a side of a library radius's reduced kernel (radius 40 at quarter resolution): the
/// blur shader's weight overrides `w0..=w10`.
pub const MAX_TAPS: usize = 10;
/// Taps a side of the full-resolution kernel the raster runs: the largest library radius.
pub const MAX_FULL_TAPS: usize = 40;

/// The radius the library blurs `asked` at: the nearest, the smaller on a tie (the QEMU path's
/// rule).
pub fn blur_radius(asked: u32) -> u32 {
    BLUR_RADII.iter().copied().min_by_key(|r| r.abs_diff(asked)).unwrap_or(20)
}

/// The resolution the library blurs at `radius` (a library radius): a pixel every `scale`
/// full-resolution pixels — half for the smallest radius, quarter from 20 on (TASK-0215 5c-3
/// P6, the stock stack's `glass-quarter`; RFC-0101 §8d).
pub const fn blur_scale(radius: u32) -> u32 {
    if radius < 20 {
        2
    } else {
        4
    }
}

/// The gaussian at `radius` (σ = R/2, at least ½), sampled a tap every `scale` pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gaussian {
    pub radius: u32,
    pub scale: u32,
}

/// A kernel's one side of normalised weights: `at(0)` the centre, `at(±j)` the tap `j` steps out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Weights {
    side: [f32; MAX_FULL_TAPS + 1],
    taps: usize,
}

impl Gaussian {
    /// The library's kernel for `asked`: the nearest library radius at its resolution.
    pub fn library(asked: u32) -> Self {
        let radius = blur_radius(asked);
        Self { radius, scale: blur_scale(radius) }
    }

    /// The full-resolution kernel at `radius` (the oracle's and the raster's); a radius past
    /// the table's largest is held to it.
    pub const fn full(radius: u32) -> Self {
        let r = if radius > MAX_FULL_TAPS as u32 { MAX_FULL_TAPS as u32 } else { radius };
        Self { radius: r, scale: 1 }
    }

    /// σ = R/2, at least one half (radius 0 blurs nothing but stays defined).
    pub fn sigma(&self) -> f32 {
        (self.radius as f32 / 2.0).max(0.5)
    }

    /// Taps a side at this resolution.
    pub const fn taps(&self) -> u32 {
        if self.scale == 0 {
            0
        } else {
            self.radius / self.scale
        }
    }

    /// The weight of a tap `d` full-resolution pixels out, before normalising: 2^(k·d²) with
    /// k = −1/(2σ² ln 2).
    fn raw(&self, d: f32) -> f32 {
        let sigma = self.sigma();
        let k = -1.0 / (2.0 * sigma * sigma * core::f32::consts::LN_2);
        libm::exp2f(k * d * d)
    }

    /// The normalised weights, one side.
    pub fn weights(&self) -> Weights {
        let taps = (self.taps() as usize).min(MAX_FULL_TAPS);
        let mut side = [0.0f32; MAX_FULL_TAPS + 1];
        let mut total = 0.0f32;
        for (j, w) in side.iter_mut().enumerate().take(taps + 1) {
            *w = self.raw((self.scale * j as u32) as f32);
            total += if j == 0 { *w } else { 2.0 * *w };
        }
        for w in side.iter_mut().take(taps + 1) {
            *w /= total;
        }
        Weights { side, taps }
    }

    /// The fetches a pass makes per output pixel: the horizontal pass one a tap, the vertical
    /// one the centre and one per pair of taps (two taps a bilinear fetch).
    pub const fn fetches(&self) -> (u32, u32) {
        let taps = self.taps();
        (2 * taps + 1, 1 + 2 * taps.div_ceil(2))
    }
}

impl Weights {
    /// Taps a side.
    pub const fn taps(&self) -> usize {
        self.taps
    }

    /// The weight of tap `j` (either side; zero past the taps).
    pub fn at(&self, j: i32) -> f32 {
        let k = j.unsigned_abs() as usize;
        if k <= self.taps {
            self.side[k]
        } else {
            0.0
        }
    }

    /// The weights from `-taps` to `taps`.
    pub fn iter(&self) -> impl Iterator<Item = (i32, f32)> + '_ {
        let taps = self.taps as i32;
        (-taps..=taps).map(move |j| (j, self.at(j)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kernel_s_weights_sum_to_one() {
        for r in BLUR_RADII {
            for g in [Gaussian::library(r), Gaussian::full(r)] {
                let sum: f32 = g.weights().iter().map(|(_, w)| w).sum();
                assert!((sum - 1.0).abs() < 1e-5, "{g:?}: {sum}");
            }
        }
    }

    #[test]
    fn the_reduced_kernel_is_the_full_one_sampled_every_scale_pixels() {
        let g = Gaussian::library(40);
        assert_eq!((g.radius, g.scale, g.taps()), (40, 4, 10));
        let (reduced, full) = (g.weights(), Gaussian::full(40).weights());
        // The same shape: the ratio of a tap to the centre is the full kernel's at 4·j.
        for j in 0..=10 {
            let (a, b) = (reduced.at(j) / reduced.at(0), full.at(4 * j) / full.at(0));
            assert!((a - b).abs() < 1e-5, "tap {j}: {a} against {b}");
        }
        assert_eq!(reduced.at(11), 0.0);
        assert!(reduced.taps() <= MAX_TAPS);
    }

    #[test]
    fn the_library_snaps_to_its_radii_and_their_resolutions() {
        assert_eq!(Gaussian::library(12), Gaussian { radius: 8, scale: 2 });
        assert_eq!(Gaussian::library(25), Gaussian { radius: 20, scale: 4 });
        assert_eq!(Gaussian::library(100), Gaussian { radius: 40, scale: 4 });
        assert_eq!(Gaussian::full(100).radius, MAX_FULL_TAPS as u32);
        assert_eq!(Gaussian { radius: 8, scale: 0 }.taps(), 0);
    }

    #[test]
    fn a_pass_s_fetches_are_its_taps_and_its_pairs() {
        // Radius 20 at quarter: five a side — eleven across, the centre and three pairs down.
        assert_eq!(Gaussian::library(20).fetches(), (11, 7));
        assert_eq!(Gaussian::library(8).fetches(), (9, 5));
        assert_eq!(Gaussian::library(40).fetches(), (21, 11));
    }
}
