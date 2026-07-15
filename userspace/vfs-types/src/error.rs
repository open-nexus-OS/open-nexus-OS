// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The stable storage error code table (RFC-0072, normative). Every
//! storage-facing response carries one of these as a `u16`; codes are
//! append-only and shared by all providers — never fork this table.
//! OWNERS: @runtime
//! STATUS: Stable codes, experimental API
//! TEST_COVERAGE: roundtrip + unknown-code tests below

/// Stable storage error codes (RFC-0072 §Contract). `0` is success.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum VfsError {
    /// Path/object does not exist.
    NotFound = 1,
    /// Denied by policy/namespace/CapFd.
    Access = 2,
    /// Write op on a read-only mount.
    ReadOnlyFs = 3,
    /// Path component is not a directory.
    NotDir = 4,
    /// File op on a directory.
    IsDir = 5,
    /// Create-exclusive target exists.
    Exists = 6,
    /// Provider out of space.
    NoSpace = 7,
    /// Size/limit cap exceeded.
    TooBig = 8,
    /// Checksum/AEAD validation failed (fail-closed).
    Integrity = 9,
    /// Object in use (e.g. open handles on remove).
    Busy = 10,
    /// Malformed request (bad name, bad cursor, bad handle).
    Invalid = 11,
    /// Op not supported by this provider/phase.
    Unsupported = 12,
    /// Underlying device error.
    Io = 13,
}

/// Wire value for success (no error).
pub const CODE_OK: u16 = 0;

impl VfsError {
    /// The stable wire code.
    #[must_use]
    pub const fn code(self) -> u16 {
        self as u16
    }

    /// Decodes a wire code; `0` (OK) and unknown codes yield `None`/`Io`.
    ///
    /// Unknown non-zero codes map to `Io` deliberately: an old client talking
    /// to a newer server must fail closed with a real error, never succeed.
    #[must_use]
    pub fn from_code(code: u16) -> Option<Self> {
        match code {
            CODE_OK => None,
            1 => Some(Self::NotFound),
            2 => Some(Self::Access),
            3 => Some(Self::ReadOnlyFs),
            4 => Some(Self::NotDir),
            5 => Some(Self::IsDir),
            6 => Some(Self::Exists),
            7 => Some(Self::NoSpace),
            8 => Some(Self::TooBig),
            9 => Some(Self::Integrity),
            10 => Some(Self::Busy),
            11 => Some(Self::Invalid),
            12 => Some(Self::Unsupported),
            _ => Some(Self::Io),
        }
    }

    /// Stable diagnostic name (used in logs/markers; never parsed).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::NotFound => "ENOTFOUND",
            Self::Access => "EACCES",
            Self::ReadOnlyFs => "EROFS",
            Self::NotDir => "ENOTDIR",
            Self::IsDir => "EISDIR",
            Self::Exists => "EEXIST",
            Self::NoSpace => "ENOSPC",
            Self::TooBig => "E2BIG",
            Self::Integrity => "EINTEGRITY",
            Self::Busy => "EBUSY",
            Self::Invalid => "EINVAL",
            Self::Unsupported => "EUNSUPPORTED",
            Self::Io => "EIO",
        }
    }
}

impl core::fmt::Display for VfsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_roundtrips() {
        for err in [
            VfsError::NotFound,
            VfsError::Access,
            VfsError::ReadOnlyFs,
            VfsError::NotDir,
            VfsError::IsDir,
            VfsError::Exists,
            VfsError::NoSpace,
            VfsError::TooBig,
            VfsError::Integrity,
            VfsError::Busy,
            VfsError::Invalid,
            VfsError::Unsupported,
            VfsError::Io,
        ] {
            assert_eq!(VfsError::from_code(err.code()), Some(err), "{err}");
        }
    }

    #[test]
    fn ok_is_no_error() {
        assert_eq!(VfsError::from_code(CODE_OK), None);
    }

    #[test]
    fn test_reject_unknown_code_fails_closed() {
        // Future/garbage codes must surface as a real error, never success.
        assert_eq!(VfsError::from_code(999), Some(VfsError::Io));
        assert_eq!(VfsError::from_code(u16::MAX), Some(VfsError::Io));
    }
}
