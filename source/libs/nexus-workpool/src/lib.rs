// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_op_in_unsafe_fn)]

//! CONTEXT: nexus-workpool — THE process-wide deterministic compute pool
//! (TASK-0276 sanctioned implementation; one shared pool, never one per
//! subsystem). A worker per CPU the process may run on (asked of the kernel, never
//! named), fence-coordinated same-AS compute threads, deterministic partition →
//! compute → canonical reduce with the `workers=1 ≡ workers=N` equality contract.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: host tests (partition math, the sizing rule, equality matrix); QEMU markers
//!   `SELFTEST: workpool determinism ok` / `… bounded ok` / `… sized ok`.
//! PUBLIC API: init(), run(), paint_bands() (the safe scoped band entry, TASK-0367 P2),
//!   MIN_BAND_ROWS, MAX_BANDS, workers(), workers_for(), chunk_bounds(), PoolError
//! DEPENDS_ON: nexus-abi (thread spawn, fences, cap transfer, the kernel's CPU answers),
//!   nexus-sched (the CPU set)
//! ADR: docs/adr/0046-deterministic-parallel-compute-workpool.md

pub mod bands;
pub mod partition;
pub mod pool;

pub use bands::{paint_bands, MAX_BANDS, MIN_BAND_ROWS};
pub use partition::chunk_bounds;
pub use pool::{init, run, workers, workers_for, JobFn, PoolError};

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

    /// A worker per CPU the process may run on, among the online ones — on one CPU as on
    /// sixty-four, and one where the kernel answers nothing.
    #[test]
    fn a_pool_has_a_worker_per_cpu_its_process_may_run_on() {
        use nexus_sched::CpuSet;
        for cpus in [1usize, 2, 4, 8, 64] {
            let online = CpuSet::first(cpus);
            assert_eq!(super::workers_for(online, online), cpus);
            // The background role: every online CPU but the first — all of one.
            let background = online.without(CpuSet::first(1));
            assert_eq!(super::workers_for(background, online), (cpus - 1).max(1), "{cpus}");
        }
        // A set naming a CPU that is not online counts the online ones.
        assert_eq!(super::workers_for(CpuSet::first(8), CpuSet::first(4)), 4);
        assert_eq!(super::workers_for(CpuSet::EMPTY, CpuSet::EMPTY), 1);
    }

    #[test]
    fn every_worker_count_a_machine_can_give_produces_identical_results() {
        for total in [1usize, 7, 64, 1000] {
            let reference = run_with_threads(total, 1);
            for workers in [2usize, 3, 4, 7, 8, 16, 64] {
                assert_eq!(
                    run_with_threads(total, workers),
                    reference,
                    "equality violated at total={total} workers={workers}"
                );
            }
        }
    }
}
