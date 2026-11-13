// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: what every CPU of the machine offers a user program (RFC-0119) — the answer to
//! the topology query `Offers`, one register: the instruction-set extensions a thread may use
//! on whichever CPU it runs, and the width of the vector registers. One definition on both
//! sides of the ABI: the kernel builds the word from the device tree and from what it keeps
//! per thread, a program reads it and chooses its code path — it never assumes an extension
//! and never executes one to find out.
//! An extension only some CPUs carry is not in the word; a register set the kernel does not
//! keep per thread is not in the word either, whatever the hardware has.
//! OWNERS: @kernel-team @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `tests` below (round trip, the dependent extensions, `test_reject_*`)
//! RFC: docs/rfcs/RFC-0119-a-thread-s-floating-point-and-vector-registers-are-its-own-kept-from-first-use.md

/// An extension a program may be offered. The discriminant is its bit in the word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Extension {
    /// Single- and double-precision floating point (F and D); a thread's registers are kept.
    Float = 0,
    /// The vector extension (V); a thread's registers are kept.
    Vector = 1,
    /// Address generation (Zba).
    Zba = 2,
    /// Basic bit manipulation (Zbb).
    Zbb = 3,
    /// Single-bit instructions (Zbs).
    Zbs = 4,
    /// Half-precision floating point (Zfh); only with [`Extension::Float`].
    Zfh = 5,
    /// Half-precision vector floating point (Zvfh); only with [`Extension::Vector`].
    Zvfh = 6,
}

impl Extension {
    /// Every extension, in bit order.
    pub const ALL: [Self; 7] =
        [Self::Float, Self::Vector, Self::Zba, Self::Zbb, Self::Zbs, Self::Zfh, Self::Zvfh];

    const fn bit(self) -> u64 {
        1 << self as u8
    }
}

const WIDTH_SHIFT: u32 = 32;
const WIDTH_MASK: u64 = 0xffff << WIDTH_SHIFT;

/// What every CPU offers: extension bits in the low half, the vector registers' width in
/// bytes in bits 32–47. A word a newer kernel sends may carry bits this type does not name;
/// they are kept and ignored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Offers(u64);

impl Offers {
    /// Nothing beyond the base instruction set.
    pub const NONE: Self = Self(0);

    /// Whether `bytes` is a width vector registers can have: a power of two of at least 16
    /// (the extension's minimum) that fits the word's field.
    #[must_use]
    pub const fn is_width(bytes: usize) -> bool {
        bytes >= 16 && bytes.is_power_of_two() && bytes <= 0x8000
    }

    /// The word of a machine: `stated(e)` — every CPU's node states `e`; `float_kept` — the
    /// kernel keeps a thread's floating-point registers; `vector_bytes` — it keeps the vector
    /// registers, of this width on every CPU. A register set that is not kept is not offered;
    /// an extension that depends on one is offered only with it; vector registers are offered
    /// only with the floating-point ones (their arithmetic uses both).
    #[must_use]
    pub fn of_machine(
        stated: impl Fn(Extension) -> bool,
        float_kept: bool,
        vector_bytes: Option<usize>,
    ) -> Self {
        let float = float_kept && stated(Extension::Float);
        let width =
            vector_bytes.filter(|&b| float && Self::is_width(b) && stated(Extension::Vector));
        let mut word = 0u64;
        for e in Extension::ALL {
            let offered = match e {
                Extension::Float => float,
                Extension::Vector => width.is_some(),
                Extension::Zfh => float && stated(e),
                Extension::Zvfh => width.is_some() && stated(e),
                Extension::Zba | Extension::Zbb | Extension::Zbs => stated(e),
            };
            if offered {
                word |= e.bit();
            }
        }
        Self(word | ((width.unwrap_or(0) as u64) << WIDTH_SHIFT))
    }

    /// The word as the syscall returns it.
    #[must_use]
    pub const fn raw(self) -> usize {
        self.0 as usize
    }

    /// A word the kernel returned. `None`: it contradicts itself — a width without the vector
    /// extension or the extension without a valid width, or a dependent extension without the
    /// one it needs. Bits this type does not name are kept.
    #[must_use]
    pub const fn from_raw(raw: usize) -> Option<Self> {
        let word = Self(raw as u64);
        let width = ((word.0 & WIDTH_MASK) >> WIDTH_SHIFT) as usize;
        let consistent = if word.has(Extension::Vector) {
            Self::is_width(width) && word.has(Extension::Float)
        } else {
            width == 0 && !word.has(Extension::Zvfh)
        };
        if !consistent || (word.has(Extension::Zfh) && !word.has(Extension::Float)) {
            return None;
        }
        Some(word)
    }

    /// Whether every CPU offers `e`.
    #[must_use]
    pub const fn has(self, e: Extension) -> bool {
        self.0 & e.bit() != 0
    }

    /// The width of a vector register in bytes on every CPU; `None` without the extension.
    #[must_use]
    pub const fn vector_bytes(self) -> Option<usize> {
        if self.has(Extension::Vector) {
            Some(((self.0 & WIDTH_MASK) >> WIDTH_SHIFT) as usize)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_machine_with_everything_round_trips() {
        let offers = Offers::of_machine(|_| true, true, Some(32));
        for e in Extension::ALL {
            assert!(offers.has(e), "{e:?}");
        }
        assert_eq!(offers.vector_bytes(), Some(32));
        assert_eq!(Offers::from_raw(offers.raw()), Some(offers));
    }

    #[test]
    fn a_register_set_the_kernel_does_not_keep_is_not_offered() {
        // The hardware has everything; the kernel keeps neither set.
        let offers = Offers::of_machine(|_| true, false, None);
        assert!(!offers.has(Extension::Float) && !offers.has(Extension::Vector));
        assert!(!offers.has(Extension::Zfh) && !offers.has(Extension::Zvfh));
        assert!(
            offers.has(Extension::Zba) && offers.has(Extension::Zbb) && offers.has(Extension::Zbs)
        );
        assert_eq!(offers.vector_bytes(), None);
    }

    #[test]
    fn vector_registers_come_only_with_the_floating_point_ones() {
        let offers = Offers::of_machine(|_| true, false, Some(32));
        assert!(!offers.has(Extension::Vector));
        assert_eq!(offers.raw() >> 32, 0, "no width without the extension");
    }

    #[test]
    fn an_extension_some_cpu_lacks_is_not_offered() {
        let offers = Offers::of_machine(|e| e != Extension::Vector, true, Some(32));
        assert!(offers.has(Extension::Float));
        assert!(!offers.has(Extension::Vector) && !offers.has(Extension::Zvfh));
        assert_eq!(offers.vector_bytes(), None);
    }

    #[test]
    fn test_reject_a_width_no_register_has() {
        for bytes in [0, 8, 24, 48, 0x1_0000] {
            assert!(!Offers::is_width(bytes), "{bytes}");
            assert!(!Offers::of_machine(|_| true, true, Some(bytes)).has(Extension::Vector));
        }
        for bytes in [16, 32, 64, 256, 0x8000] {
            assert!(Offers::is_width(bytes), "{bytes}");
        }
    }

    #[test]
    fn test_reject_a_word_that_contradicts_itself() {
        let float = 1usize << Extension::Float as u8;
        let vector = 1usize << Extension::Vector as u8;
        let zfh = 1usize << Extension::Zfh as u8;
        let zvfh = 1usize << Extension::Zvfh as u8;
        // A width without the extension, the extension without a width, or with an odd one.
        assert_eq!(Offers::from_raw(float | (32 << 32)), None);
        assert_eq!(Offers::from_raw(float | vector), None);
        assert_eq!(Offers::from_raw(float | vector | (24 << 32)), None);
        // Vector registers without the floating-point ones; dependents without their base.
        assert_eq!(Offers::from_raw(vector | (32 << 32)), None);
        assert_eq!(Offers::from_raw(zfh), None);
        assert_eq!(Offers::from_raw(float | zvfh), None);
        assert!(Offers::from_raw(float | vector | zvfh | (16 << 32)).is_some());
    }

    #[test]
    fn a_newer_kernels_unknown_bits_are_kept_and_ignored() {
        let raw = (1usize << 20) | (1 << Extension::Zbb as u8);
        let offers = Offers::from_raw(raw).expect("consistent");
        assert!(offers.has(Extension::Zbb));
        assert_eq!(offers.raw(), raw);
    }
}
