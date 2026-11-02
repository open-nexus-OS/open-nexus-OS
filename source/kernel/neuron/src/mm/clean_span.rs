// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The range rule of `vmo_clean` (syscall 74, ADR-0092) as a pure predicate — NOT
//! target-gated, so its reject matrix runs on the host: the range lies inside the capability the
//! caller holds, and only a writable object (`Vmo`, not a read-only alias, not sealed) is
//! cleaned — cleaning is maintenance on memory the caller may write, never a probe of shared
//! read-only pages (a flush+reload side channel over another process's image).
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host (`tests` below); QEMU `KSELFTEST: vmo clean ok (`
//! ADR: docs/adr/0092-a-surface-is-a-buffer-the-app-draws-and-the-gpu-reads-gpud-allocates-it-for-its-owner-windowd-composes-by-reference.md

/// Why a `vmo_clean` is refused before any cache is touched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanRefusal {
    /// A read-only alias or a sealed object: the caller may not write it, so it may not clean it.
    NotWritable,
    /// The range leaves the capability (or overflows).
    OutOfRange,
}

/// Whether `[offset, offset + len)` of an object whose capability spans `cap_len` bytes may be
/// cleaned by a caller holding it `writable` (a `Vmo`, not sealed). An empty range is admitted
/// (nothing to do).
pub fn admits(
    writable: bool,
    offset: usize,
    len: usize,
    cap_len: usize,
) -> Result<(), CleanRefusal> {
    if !writable {
        return Err(CleanRefusal::NotWritable);
    }
    match offset.checked_add(len) {
        Some(end) if end <= cap_len => Ok(()),
        _ => Err(CleanRefusal::OutOfRange),
    }
}

#[cfg(test)]
mod tests {
    use super::{admits, CleanRefusal};

    #[test]
    fn test_reject_vmo_clean_read_only_alias() {
        assert_eq!(admits(false, 0, 4096, 8192), Err(CleanRefusal::NotWritable));
    }

    #[test]
    fn test_reject_vmo_clean_past_end() {
        assert_eq!(admits(true, 4096, 4097, 8192), Err(CleanRefusal::OutOfRange));
        assert_eq!(admits(true, 8193, 0, 8192), Err(CleanRefusal::OutOfRange));
        assert_eq!(admits(true, usize::MAX, 2, usize::MAX), Err(CleanRefusal::OutOfRange));
    }

    #[test]
    fn a_range_inside_the_capability_is_admitted() {
        assert_eq!(admits(true, 0, 8192, 8192), Ok(()));
        assert_eq!(admits(true, 4096, 0, 8192), Ok(()));
        assert_eq!(admits(true, 100, 7, 8192), Ok(()));
    }
}
