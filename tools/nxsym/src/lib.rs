// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `nxsym` host symbolization library: Build-ID extraction (GNU note
//! with a documented deterministic fallback), a CBOR symbol index keyed by
//! Build-ID, and bounded address lookup. Lookup callers pass the Build-ID they
//! read from crash artifacts (`MinidumpFrame.build_id` / `.nxcd` frames) —
//! this crate never re-derives ids for dumps.
//! OWNERS: @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Unit tests per module plus `tests/crashdump_v2_host` integration suite
//! ADR: tasks/TASK-0048-crashdump-v2a-host-pipeline-nxsym-nx-crash.md

#![forbid(unsafe_code)]

pub mod build_id;
pub mod index;

pub use build_id::{build_id_for_elf, fallback_build_id, BuildIdSource};
pub use index::{
    build_index, lookup, read_index, write_index, BinaryIndex, ResolvedFrame, SymbolEntry,
    SymbolIndex,
};

/// Deterministic reject reasons for nxsym operations. Index files and ELF
/// inputs are untrusted; every failure maps to exactly one variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NxsymError {
    /// Input file could not be read.
    Io,
    /// Input exceeds the configured size bound.
    OversizeInput,
    /// ELF parsing failed.
    ElfParse,
    /// DWARF/loader setup failed for an ELF being indexed.
    LoaderOpen,
    /// Index magic/version mismatch.
    BadIndexMagic,
    /// Index CBOR payload failed to decode.
    IndexDecode,
    /// Index CBOR payload failed to encode.
    IndexEncode,
    /// Index violates its own invariants (ordering, caps, id syntax).
    IndexInvariant,
    /// Two indexed binaries share the same Build-ID.
    DuplicateBuildId,
    /// Requested Build-ID is not present in the index.
    UnknownBuildId,
}

impl core::fmt::Display for NxsymError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let label = match self {
            Self::Io => "io failure",
            Self::OversizeInput => "oversize input",
            Self::ElfParse => "elf parse failure",
            Self::LoaderOpen => "dwarf loader failure",
            Self::BadIndexMagic => "bad index magic",
            Self::IndexDecode => "index decode failure",
            Self::IndexEncode => "index encode failure",
            Self::IndexInvariant => "index invariant violation",
            Self::DuplicateBuildId => "duplicate build id",
            Self::UnknownBuildId => "unknown build id",
        };
        write!(f, "{label}")
    }
}

impl std::error::Error for NxsymError {}
