// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the painter's kernels — the two loops a frame spends its flat pixels in: a span set
//! to one pixel ([`fill`]) and one colour laid over a span, source over ([`blend`]). Each is a
//! scalar function ([`reference`]) and, on a hart with the vector unit, a vector form that
//! leaves the same bytes (ADR-0099 rule 4). A program states once what the kernel reported
//! ([`choose`]); until it does, and on every other machine, the scalar form runs.
//!
//! WHO CALLS: the row painter's spans (`nexus-scene-raster`: a box's interior, a glass reset)
//! and app-host's grounds (the base a row starts from) — so apps, motion layers and animations
//! are painted through here without knowing it.
//!
//! WHAT A VECTOR FORM COSTS: the first vector instruction of a thread makes the kernel keep 32
//! registers of the hart's width for it (RFC-0119). A span shorter than [`SHORT`] never asks
//! for that: it stays scalar.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: the unit tests below (the reference against the painter's per-pixel
//! arithmetic over every alpha, destination and colour; the division the vector form uses over
//! every sum); the vector forms against the reference on the machine: `SELFTEST: paint kernels
//! ok` (QEMU 16-byte registers, the board 32) and the measurement program under the user-mode
//! emulator at 16, 32, 64 and 128 bytes.
//! ADR: docs/adr/0099-every-userspace-program-is-built-for-one-hard-float-target-vectors-are-asked-for-at-run-time.md

#![cfg_attr(not(test), no_std)]
// `deny` (not `forbid`) so ONLY `riscv.rs` — the assembly and its two calls — may opt in.
#![deny(unsafe_code)]

pub mod proof;
pub mod reference;
#[cfg(target_arch = "riscv64")]
mod riscv;

use core::sync::atomic::{AtomicU8, Ordering};

/// The unit a kernel runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    /// The scalar reference.
    Scalar,
    /// The vector unit, at whatever width its registers have.
    Vector,
}

/// What [`choose`] left: 0 the scalar form (also: never chosen), 1 the vector form.
static CHOSEN: AtomicU8 = AtomicU8::new(0);

/// A span below this many bytes is painted by the scalar form whatever was chosen: setting the
/// unit up costs more than sixteen pixels do, and a thread that paints nothing longer never
/// makes the kernel keep vector registers for it.
pub const SHORT: usize = 64;

/// States what the kernel reported for this program's harts (`nexus_abi::sched::offers()`), once,
/// before the first frame; every thread of the process paints with it from then on. Returns what
/// runs: [`Unit::Vector`] only when it was offered and this build carries vector forms.
///
/// The claim is the caller's: on a hart without the vector unit the first long span ends the
/// program with an illegal instruction — which is why the kernel's report is the only source.
pub fn choose(offered: Unit) -> Unit {
    let runs = if cfg!(target_arch = "riscv64") { offered } else { Unit::Scalar };
    CHOSEN.store(u8::from(runs == Unit::Vector), Ordering::Relaxed);
    runs
}

/// What the kernels run on.
#[must_use]
pub fn chosen() -> Unit {
    if CHOSEN.load(Ordering::Relaxed) == 1 {
        Unit::Vector
    } else {
        Unit::Scalar
    }
}

/// Every whole pixel of `row` becomes `px` ([`reference::fill`]).
#[inline]
pub fn fill(row: &mut [u8], px: [u8; 4]) {
    #[cfg(target_arch = "riscv64")]
    if row.len() >= SHORT && chosen() == Unit::Vector {
        return riscv::fill(row, px);
    }
    reference::fill(row, px);
}

/// The straight colour `bgra` (blue, green, red, alpha) source-over every whole pixel of `row`,
/// premultiplied BGRA ([`reference::blend`]). A clear colour changes nothing and an opaque one
/// is a [`fill`] — both are what the arithmetic yields, taken without it.
#[inline]
pub fn blend(row: &mut [u8], bgra: [u8; 4]) {
    match bgra[3] {
        0 => {}
        255 => fill(row, bgra),
        _ => {
            #[cfg(target_arch = "riscv64")]
            if row.len() >= SHORT && chosen() == Unit::Vector {
                return riscv::blend(row, &reference::Over::of(bgra));
            }
            reference::blend(row, bgra);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::reference::{self, Over};
    use super::{blend, choose, chosen, fill, Unit};

    /// The painter's own pixel before this crate (`RowCanvas::blend`), kept here as the witness.
    fn painters_pixel(dst: [u8; 4], [b, g, r, a]: [u8; 4]) -> [u8; 4] {
        let (a, inv) = (u32::from(a), 255 - u32::from(a));
        let mix = |d: u8, s: u8| ((u32::from(d) * inv + u32::from(s) * a) / 255) as u8;
        [mix(dst[0], b), mix(dst[1], g), mix(dst[2], r), (a + u32::from(dst[3]) * inv / 255) as u8]
    }

    #[test]
    fn the_reference_is_the_painters_pixel_for_every_alpha_destination_and_colour() {
        for a in 0..=255u8 {
            let over = Over::of([0, 0, 0, a]);
            for d in 0..=255u8 {
                // The alpha channel: `(d · inv + 255 · a) / 255` is `a + d · inv / 255`.
                assert_eq!(over.channel(d, 3), painters_pixel([0, 0, 0, d], [0, 0, 0, a])[3]);
                for c in 0..=255u8 {
                    let over = Over::of([c, 0, 0, a]);
                    assert_eq!(over.channel(d, 0), painters_pixel([d, 0, 0, 0], [c, 0, 0, a])[0]);
                }
            }
        }
    }

    #[test]
    fn no_sum_passes_sixteen_bits_and_the_multiply_divides_it_exactly() {
        // What a vector form relies on: the largest sum is 255 · 255 …
        for a in 0..=255u8 {
            let over = Over::of([255, 255, 255, a]);
            for k in 0..4 {
                assert!(255 * u32::from(over.inv) + u32::from(over.add[k]) <= 255 * 255);
            }
        }
        // … and the high half of a multiply by 0x8081, shifted by seven, is the division by 255
        // for every 16-bit value.
        for sum in 0..=u32::from(u16::MAX) {
            assert_eq!((sum * 0x8081) >> 23, sum / 255, "sum={sum}");
        }
    }

    #[test]
    fn a_fill_sets_whole_pixels_and_leaves_the_rest() {
        let mut row = [9u8; 4 * 40 + 3];
        fill(&mut row, [1, 2, 3, 4]);
        assert!(row[..160].chunks_exact(4).all(|px| px == [1, 2, 3, 4]));
        assert_eq!(row[160..], [9, 9, 9]);
    }

    #[test]
    fn a_clear_colour_changes_nothing_and_an_opaque_one_is_a_fill_as_the_arithmetic_says() {
        let mut seed = 0x9e37_79b9u32;
        let mut row = [0u8; 4 * 33 + 2];
        for b in &mut row {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *b = (seed >> 24) as u8;
        }
        for colour in [[7, 8, 9, 0], [7, 8, 9, 255], [200, 100, 50, 128], [255, 255, 255, 1]] {
            let (mut short, mut long) = (row, row);
            blend(&mut short, colour);
            reference::blend(&mut long, colour);
            assert_eq!(short, long, "colour={colour:?}");
        }
    }

    #[test]
    fn the_proofs_digest_is_the_references() {
        // On the host the kernels ARE the reference: this pins the number a machine with a
        // vector unit has to print.
        assert_eq!(super::proof::run(), Ok(super::proof::DIGEST));
    }

    #[test]
    #[cfg(not(target_arch = "riscv64"))]
    fn a_machine_without_vector_forms_stays_scalar_whatever_is_offered() {
        assert_eq!(choose(Unit::Vector), Unit::Scalar);
        assert_eq!(chosen(), Unit::Scalar);
    }
}
