// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Offline fsck for nxfs containers (RFC-0071): validates
//! superblocks, checkpoint slots and the journal, reports discarded orphan
//! tails, and (with `repair`) folds the recovered state into a fresh
//! checkpoint so the orphan bytes are retired. Repair NEVER invents data —
//! it only discards what replay already proved uncommitted.
//! OWNERS: @runtime
//! STATUS: Experimental (TASK-0292)
//! TEST_COVERAGE: exit-code matrix in tests/crash_injection.rs

use storage::BlockDevice;

use crate::fs::Nxfs;
use crate::NxfsError;

/// fsck outcome, mapped to stable exit codes by the CLI tool
/// (0 = clean, 1 = repaired, 2 = unrecoverable).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsckOutcome {
    /// Mounts cleanly, no discarded journal tail.
    Clean,
    /// An orphan/torn journal tail was found; with `repair` it was retired
    /// via a fresh checkpoint, without `repair` it is only reported.
    Repaired,
    /// No valid superblock/checkpoint combination mounts.
    Unrecoverable,
}

/// Deterministic fsck report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FsckReport {
    pub outcome: FsckOutcome,
    /// True when a torn/orphaned journal tail was discarded by replay.
    pub orphan_tail: bool,
    /// True when `repair` rewrote a checkpoint to retire the tail.
    pub repaired: bool,
}

/// Validates the container. With `repair`, a discarded orphan tail is retired
/// by forcing a checkpoint of the recovered (committed-only) state.
pub fn fsck<D: BlockDevice>(device: D, repair: bool) -> (FsckReport, Option<D>) {
    let mut fs = match Nxfs::mount(device) {
        Ok(fs) => fs,
        Err(NxfsError::Io) | Err(NxfsError::Integrity) => {
            return (
                FsckReport {
                    outcome: FsckOutcome::Unrecoverable,
                    orphan_tail: false,
                    repaired: false,
                },
                None,
            );
        }
        Err(_) => {
            return (
                FsckReport {
                    outcome: FsckOutcome::Unrecoverable,
                    orphan_tail: false,
                    repaired: false,
                },
                None,
            );
        }
    };
    let orphan = fs.replay_discarded_tail;
    if !orphan {
        return (
            FsckReport { outcome: FsckOutcome::Clean, orphan_tail: false, repaired: false },
            Some(fs.into_device()),
        );
    }
    let repaired = repair && fs.write_checkpoint().is_ok();
    (
        FsckReport { outcome: FsckOutcome::Repaired, orphan_tail: true, repaired },
        Some(fs.into_device()),
    )
}
