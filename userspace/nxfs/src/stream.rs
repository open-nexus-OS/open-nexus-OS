// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Extent-window arithmetic for STREAMING reads and windowed
//! copy-on-write writes (TASK-0179; retires the whole-file `materialize`
//! path that capped files at 4 MiB and made every op O(file) in RAM).
//! Pure block math over the extent list — host-tested exhaustively; the
//! engine (`fs.rs`) supplies device IO and the journal commit. Crash
//! discipline is unchanged and load-bearing: new content lands ONLY in
//! fresh blocks, the journaled `Op::Write` commits the spliced extent
//! list atomically, and `state.apply` reconciles the block bitmap (blocks
//! present in both old and new lists stay used; replaced ones return to
//! the free pool).
//! OWNERS: @runtime
//! STATUS: Experimental (TASK-0179)
//! PUBLIC API: runs_for_window(), split_at_block(), splice()
//! TEST_COVERAGE: unit tests below + fs.rs streaming integration tests
//! ADR: docs/adr/0043-user-data-in-dedicated-cow-fs-statefs-stays-service-kv.md

use alloc::vec::Vec;

use crate::state::Extent;

/// One contiguous device run backing part of a file window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Run {
    /// First device logical block of the run.
    pub lb: u64,
    /// Blocks in the run.
    pub blocks: u64,
    /// File-relative block index of the run's first block.
    pub file_block: u64,
}

/// Maps the file-block window `[first_block, first_block + blocks)` onto
/// the extent list as contiguous device runs (in file order). Windows
/// beyond the extent list are simply not covered — the caller bounds
/// against the object size first.
pub(crate) fn runs_for_window(extents: &[Extent], first_block: u64, blocks: u64) -> Vec<Run> {
    let mut runs = Vec::new();
    if blocks == 0 {
        return runs;
    }
    let window_end = first_block + blocks;
    let mut cursor = 0u64;
    for extent in extents {
        let extent_blocks = u64::from(extent.blocks);
        let extent_end = cursor + extent_blocks;
        if extent_end > first_block && cursor < window_end {
            let from = first_block.max(cursor);
            let to = window_end.min(extent_end);
            runs.push(Run { lb: extent.lb + (from - cursor), blocks: to - from, file_block: from });
        }
        cursor = extent_end;
        if cursor >= window_end {
            break;
        }
    }
    runs
}

/// Splits an extent list at a file-block boundary: `(prefix, suffix)`
/// where the prefix covers blocks `[0, at)` and the suffix `[at, ..)`.
/// An extent straddling the boundary is divided (same device blocks, two
/// list entries — no data moves).
pub(crate) fn split_at_block(extents: &[Extent], at: u64) -> (Vec<Extent>, Vec<Extent>) {
    let mut prefix = Vec::new();
    let mut suffix = Vec::new();
    let mut cursor = 0u64;
    for extent in extents {
        let extent_blocks = u64::from(extent.blocks);
        let extent_end = cursor + extent_blocks;
        if extent_end <= at {
            prefix.push(*extent);
        } else if cursor >= at {
            suffix.push(*extent);
        } else {
            let head = at - cursor;
            prefix.push(Extent { lb: extent.lb, blocks: head as u32 });
            suffix.push(Extent { lb: extent.lb + head, blocks: (extent_blocks - head) as u32 });
        }
        cursor = extent_end;
    }
    (prefix, suffix)
}

/// Builds the spliced extent list for a windowed CoW write: the old
/// blocks `[0, first_block)` are kept, `fresh` replaces the touched range,
/// and old blocks `[touched_end, ..)` are kept when the file extends past
/// the window. Adjacent device-contiguous entries are coalesced so the
/// list does not grow unboundedly under repeated window writes.
pub(crate) fn splice(
    old: &[Extent],
    first_block: u64,
    touched_end: u64,
    fresh: &[Extent],
) -> Vec<Extent> {
    let (prefix, _) = split_at_block(old, first_block);
    let (_, suffix) = split_at_block(old, touched_end);
    let mut out = Vec::with_capacity(prefix.len() + fresh.len() + suffix.len());
    for extent in prefix.iter().chain(fresh.iter()).chain(suffix.iter()) {
        push_coalesced(&mut out, *extent);
    }
    out
}

fn push_coalesced(list: &mut Vec<Extent>, extent: Extent) {
    if extent.blocks == 0 {
        return;
    }
    if let Some(last) = list.last_mut() {
        if last.lb + u64::from(last.blocks) == extent.lb {
            // Same device run — merge (u32 overflow is unreachable at the
            // 64 MiB file cap, but saturate defensively).
            let merged = u64::from(last.blocks) + u64::from(extent.blocks);
            if let Ok(merged) = u32::try_from(merged) {
                last.blocks = merged;
                return;
            }
        }
    }
    list.push(extent);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ext(lb: u64, blocks: u32) -> Extent {
        Extent { lb, blocks }
    }

    #[test]
    fn window_runs_cover_exactly_the_window() {
        // File blocks: [0..4) on lb 100, [4..6) on lb 500, [6..9) on lb 40.
        let extents = [ext(100, 4), ext(500, 2), ext(40, 3)];
        assert_eq!(
            runs_for_window(&extents, 0, 9),
            alloc::vec![
                Run { lb: 100, blocks: 4, file_block: 0 },
                Run { lb: 500, blocks: 2, file_block: 4 },
                Run { lb: 40, blocks: 3, file_block: 6 },
            ]
        );
        // Interior window straddling two extents.
        assert_eq!(
            runs_for_window(&extents, 3, 2),
            alloc::vec![
                Run { lb: 103, blocks: 1, file_block: 3 },
                Run { lb: 500, blocks: 1, file_block: 4 },
            ]
        );
        // Window past the list: uncovered (caller bounds by size).
        assert_eq!(runs_for_window(&extents, 9, 4), alloc::vec![]);
        assert_eq!(runs_for_window(&extents, 2, 0), alloc::vec![]);
    }

    #[test]
    fn split_divides_straddling_extents_without_moving_blocks() {
        let extents = [ext(100, 4), ext(500, 2)];
        let (prefix, suffix) = split_at_block(&extents, 2);
        assert_eq!(prefix, alloc::vec![ext(100, 2)]);
        assert_eq!(suffix, alloc::vec![ext(102, 2), ext(500, 2)]);
        // Boundary on an extent edge: clean partition.
        let (prefix, suffix) = split_at_block(&extents, 4);
        assert_eq!(prefix, alloc::vec![ext(100, 4)]);
        assert_eq!(suffix, alloc::vec![ext(500, 2)]);
        // At zero / past the end.
        let (prefix, suffix) = split_at_block(&extents, 0);
        assert!(prefix.is_empty());
        assert_eq!(suffix.len(), 2);
        let (prefix, suffix) = split_at_block(&extents, 99);
        assert_eq!(prefix.len(), 2);
        assert!(suffix.is_empty());
    }

    #[test]
    fn splice_replaces_the_window_and_coalesces() {
        let old = [ext(100, 8)];
        // Replace file blocks [2..5) with fresh lb 300.
        let spliced = splice(&old, 2, 5, &[ext(300, 3)]);
        assert_eq!(spliced, alloc::vec![ext(100, 2), ext(300, 3), ext(105, 3)]);
        // Fresh blocks device-identical to the replaced range coalesce the
        // WHOLE list back into one run (prefix + fresh + suffix are all
        // device-contiguous) — in-place-shaped CoW costs no fragmentation.
        let spliced = splice(&old, 2, 5, &[ext(102, 3)]);
        assert_eq!(spliced, alloc::vec![ext(100, 8)]);
        // Append shape: window entirely past the old list.
        let spliced = splice(&old, 8, 12, &[ext(700, 4)]);
        assert_eq!(spliced, alloc::vec![ext(100, 8), ext(700, 4)]);
    }
}
