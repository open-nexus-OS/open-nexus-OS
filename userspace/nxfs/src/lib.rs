// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The nxfs user-data filesystem engine (RFC-0071 Phase 1) —
//! crash-atomic transactions over a bounded metadata journal, dual checkpoint
//! slots (a torn checkpoint can never brick the container), crc32c on every
//! metadata structure, canonical byte-ordered directory listings. Pure
//! host-first library over the `storage::BlockDevice` trait; `nxfsd`
//! (TASK-0293) is a thin service shell. No clocks, no randomness — callers
//! inject timestamps, replay is byte-deterministic.
//! OWNERS: @runtime
//! STATUS: Experimental (TASK-0292)
//! API_STABILITY: Unstable (on-disk format v1 per RFC-0071)
//! TEST_COVERAGE: module unit tests + tests/ crash-injection suite

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

mod checkpoint;
mod dev;
mod format;
mod fs;
mod fsck;
mod journal;
mod state;

/// Diagnostic mount-step tracer (feature `trace`); a no-op otherwise.
macro_rules! nxfs_trace {
    ($msg:literal) => {{
        #[cfg(feature = "trace")]
        {
            let _ = nexus_abi::debug_write(concat!($msg, "\n").as_bytes());
        }
    }};
}
pub(crate) use nxfs_trace;

pub use format::{Uuid, LOGICAL_BLOCK_SIZE, MAX_DEPTH, MAX_NAME_LEN, NXFS_VERSION};
pub use fs::{MkfsOptions, Nxfs};
pub use fsck::{fsck, FsckOutcome, FsckReport};
pub use nexus_vfs_types::{DirEntry, FileKind, ReadDirPage, VfsError};

use core::fmt;

/// Errors surfaced by the engine. Maps 1:1 onto the RFC-0072 stable codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NxfsError {
    /// Path/object does not exist.
    NotFound,
    /// Path component is not a directory.
    NotDir,
    /// File op on a directory.
    IsDir,
    /// Create-exclusive target exists.
    Exists,
    /// Container out of space (blocks or journal).
    NoSpace,
    /// Size/limit cap exceeded (name/depth/value bounds).
    TooBig,
    /// Checksum validation failed (fail-closed).
    Integrity,
    /// Object in use (non-empty directory on remove/replace).
    Busy,
    /// Malformed input (bad name, bad path, bad cursor).
    Invalid,
    /// Underlying device error.
    Io,
    /// Op not supported in this phase (e.g. snapshots before Phase 3).
    Unsupported,
}

impl NxfsError {
    /// The stable RFC-0072 wire error.
    #[must_use]
    pub fn to_vfs(self) -> VfsError {
        match self {
            Self::NotFound => VfsError::NotFound,
            Self::NotDir => VfsError::NotDir,
            Self::IsDir => VfsError::IsDir,
            Self::Exists => VfsError::Exists,
            Self::NoSpace => VfsError::NoSpace,
            Self::TooBig => VfsError::TooBig,
            Self::Integrity => VfsError::Integrity,
            Self::Busy => VfsError::Busy,
            Self::Invalid => VfsError::Invalid,
            Self::Io => VfsError::Io,
            Self::Unsupported => VfsError::Unsupported,
        }
    }
}

impl fmt::Display for NxfsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.to_vfs().name())
    }
}

/// Result alias for engine operations.
pub type Result<T> = core::result::Result<T, NxfsError>;
