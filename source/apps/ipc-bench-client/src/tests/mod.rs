// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Test implementations for IPC benchmark suite

extern crate alloc;

pub mod latency;
pub mod queue_pressure;
pub mod bulk;
pub mod mixed;
pub mod determinism;

use alloc::vec::Vec;
use nexus_abi::nsec;
use core::sync::atomic::{AtomicU32, Ordering};

static ENDPOINT_A: AtomicU32 = AtomicU32::new(0);
static ENDPOINT_B: AtomicU32 = AtomicU32::new(0);

pub fn set_endpoints(ep_a: u32, ep_b: u32) {
    ENDPOINT_A.store(ep_a, Ordering::SeqCst);
    ENDPOINT_B.store(ep_b, Ordering::SeqCst);
}

pub fn get_endpoints() -> (u32, u32) {
    (ENDPOINT_A.load(Ordering::SeqCst), ENDPOINT_B.load(Ordering::SeqCst))
}

/// Calibrate nsec() syscall overhead by measuring empty timing loops
pub fn calibrate_nsec_overhead() -> u64 {
    let iterations = 100_000;
    let mut deltas = Vec::with_capacity(iterations);

    for _ in 0..iterations {
        let start = nsec().unwrap_or(0);
        let end = nsec().unwrap_or(0);
        deltas.push(end.saturating_sub(start));
    }

    // Compute median (p50)
    deltas.sort_unstable();
    deltas[iterations / 2]
}

/// Compute average from slice
pub fn compute_avg(values: &[u64]) -> u64 {
    if values.is_empty() {
        return 0;
    }
    let sum: u64 = values.iter().sum();
    sum / values.len() as u64
}
