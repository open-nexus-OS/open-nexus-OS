// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_op_in_unsafe_fn)]

//! CONTEXT: nexus-workpool — THE process-wide deterministic compute pool
//! (TASK-0276 sanctioned implementation; one shared pool, never one per
//! subsystem). Fixed worker count, fence-coordinated same-AS compute
//! threads, deterministic partition → compute → canonical reduce with the
//! `workers=1 ≡ workers=N` equality contract.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: host tests (partition math, equality matrix); QEMU markers
//!   `SELFTEST: workpool determinism ok` / `SELFTEST: workpool bounded ok`.
//! PUBLIC API: init(), run(), chunk_bounds(), PoolError, MAX_WORKERS
//! DEPENDS_ON: nexus-abi (thread spawn, fences, cap transfer)
//! ADR: docs/adr/0016-kernel-libs-architecture.md

pub mod partition;
pub mod pool;

pub use partition::chunk_bounds;
pub use pool::{init, run, JobFn, PoolError, MAX_WORKERS};

#[cfg(test)]
mod equality_matrix {
    //! TASK-0276 proof: for a pure per-element job, the result is identical
    //! for every worker count — including REAL std threads on the host.

    use super::chunk_bounds;

    fn transform(i: usize) -> u64 {
        (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_left(13) ^ 0xA5A5
    }

    fn run_with_threads(total: usize, workers: usize) -> Vec<u64> {
        let out: Vec<std::sync::atomic::AtomicU64> =
            (0..total).map(|_| std::sync::atomic::AtomicU64::new(0)).collect();
        std::thread::scope(|scope| {
            for idx in 0..workers {
                let out = &out;
                scope.spawn(move || {
                    let (start, end) = chunk_bounds(total, workers, idx);
                    for i in start..end {
                        out[i].store(transform(i), std::sync::atomic::Ordering::Relaxed);
                    }
                });
            }
        });
        out.iter().map(|v| v.load(std::sync::atomic::Ordering::Relaxed)).collect()
    }

    #[test]
    fn workers_1_to_4_produce_identical_results() {
        for total in [1usize, 7, 64, 1000] {
            let reference = run_with_threads(total, 1);
            for workers in 2usize..=4 {
                assert_eq!(
                    run_with_threads(total, workers),
                    reference,
                    "equality violated at total={total} workers={workers}"
                );
            }
        }
    }
}
