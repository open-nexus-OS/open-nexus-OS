// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Directory-entry types and the RFC-0072 bounds every VFS surface
//! (server, client, app-host) must agree on.
//! OWNERS: @runtime
//! STATUS: Experimental (TASK-0291)
//! TEST_COVERAGE: bounds asserted in wire.rs roundtrip tests

use alloc::string::String;

/// Maximum entries a single ReadDir page may carry (RFC-0072).
pub const MAX_ENTRIES_PER_PAGE: u16 = 64;
/// Maximum entry-name length in bytes (RFC-0072).
pub const MAX_NAME_LEN: usize = 255;
/// Maximum request path length in bytes (RFC-0072).
pub const MAX_PATH_LEN: usize = 1024;

/// Entry kind on the wire (`u16` in stat; `u8` in readdir pages).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FileKind {
    /// Regular file.
    File = 0,
    /// Directory.
    Dir = 1,
}

impl FileKind {
    /// Decodes the wire byte; unknown kinds are rejected (fail-closed).
    #[must_use]
    pub fn from_wire(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::File),
            1 => Some(Self::Dir),
            _ => None,
        }
    }

    /// Stable lowercase label used on app-facing surfaces (`svc.files`).
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Dir => "dir",
        }
    }
}

/// One directory entry as served by ReadDir.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// Entry name (single path segment, never contains `/`).
    pub name: String,
    /// Entry kind.
    pub kind: FileKind,
    /// Size in bytes (0 for directories).
    pub size: u64,
}
