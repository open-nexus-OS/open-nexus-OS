// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Deterministic boot knobs shared across the kernel.
//!
//! The NEURON bring-up environment runs both on the host and inside QEMU.
//! For reproducibility the kernel exposes a deterministic seed and a fixed
//! timer quantum that higher level code (including selftests) can consume.

use core::sync::atomic::{AtomicU64, Ordering};

const DEFAULT_SEED: u64 = 0x6e6575726f6e; // ASCII "neuron"
const DEFAULT_TICK_NS: u64 = 1_000_000; // 1 ms slice

static SEED: AtomicU64 = AtomicU64::new(DEFAULT_SEED);
static FIXED_TICK_NS: AtomicU64 = AtomicU64::new(DEFAULT_TICK_NS);

/// Returns the deterministic seed for pseudo random number generators.
#[inline]
pub fn seed() -> u64 {
    SEED.load(Ordering::Relaxed)
}

/// Overrides the deterministic seed.
///
/// This is primarily used by unit tests to exercise different execution
/// paths while still allowing reproducible runs.
#[inline]
pub fn set_seed(value: u64) {
    SEED.store(value, Ordering::Relaxed);
}

/// Returns the fixed timer quantum used for deterministic scheduling.
#[inline]
pub fn fixed_tick_ns() -> u64 {
    FIXED_TICK_NS.load(Ordering::Relaxed)
}

/// Overrides the fixed timer quantum in nanoseconds.
#[inline]
pub fn set_fixed_tick_ns(value: u64) {
    FIXED_TICK_NS.store(value, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_roundtrip() {
        set_seed(42);
        assert_eq!(seed(), 42);
    }

    #[test]
    fn tick_roundtrip() {
        set_fixed_tick_ns(1234);
        assert_eq!(fixed_tick_ns(), 1234);
    }
}
