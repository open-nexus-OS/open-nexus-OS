// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Deterministic contiguous partitioning (TASK-0276) — pure math,
//! host-tested; the SSOT for how a workload of `total` items is split across
//! `workers` chunks. Chunk boundaries depend ONLY on (total, workers, idx),
//! never on timing — the root of the `workers=1 ≡ workers=N` equality proof.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: host unit tests (coverage, disjointness, determinism)
//! ADR: docs/adr/0046-deterministic-parallel-compute-workpool.md

/// Half-open range `[start, end)` of chunk `idx` when splitting `total`
/// items across `workers` contiguous chunks. Remainder items go to the
/// lowest-indexed chunks (deterministic).
#[must_use]
pub const fn chunk_bounds(total: usize, workers: usize, idx: usize) -> (usize, usize) {
    if workers == 0 || idx >= workers {
        return (0, 0);
    }
    let base = total / workers;
    let rem = total % workers;
    let extra_before = if idx < rem { idx } else { rem };
    let start = idx * base + extra_before;
    let len = base + if idx < rem { 1 } else { 0 };
    (start, start + len)
}

#[cfg(test)]
mod tests {
    use super::chunk_bounds;

    #[test]
    fn chunks_cover_exactly_and_disjointly() {
        for total in [0usize, 1, 7, 64, 1000, 1023] {
            for workers in 1usize..=4 {
                let mut next = 0usize;
                for idx in 0..workers {
                    let (start, end) = chunk_bounds(total, workers, idx);
                    assert_eq!(start, next, "gap/overlap at total={total} w={workers} i={idx}");
                    assert!(end >= start);
                    next = end;
                }
                assert_eq!(next, total, "coverage at total={total} w={workers}");
            }
        }
    }

    #[test]
    fn degenerate_inputs_are_empty() {
        assert_eq!(chunk_bounds(10, 0, 0), (0, 0));
        assert_eq!(chunk_bounds(10, 2, 5), (0, 0));
    }
}
