// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Crashdump v2 `.nxcd` container format: bounded named sections,
//! deterministic encode/decode, minidump-v1 conversion, GC planning, and an
//! optional zstd wrapper (`.nxcd.zst`, feature `zst`, host tools only).
//! OWNERS: @reliability
//! STATUS: Functional (host-first; OS ingestion is TASK-0049)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Unit tests per module plus `tests/crashdump_v2_host` integration suite
//! ADR: tasks/TASK-0048-crashdump-v2a-host-pipeline-nxsym-nx-crash.md

#![forbid(unsafe_code)]

pub mod container;
pub mod gc;
pub mod sections;
#[cfg(feature = "zst")]
pub mod zst;

pub use container::{NxcdContainer, SectionKind, MAX_TOTAL_NXCD};
pub use gc::{plan_purge, GcBudget, GcEntry};
pub use sections::{from_minidump, CrashHeader, FrameRecord, FramesSection, MapsSection};
#[cfg(feature = "zst")]
pub use zst::{compress_nxcd, decompress_nxcd};

/// Deterministic reject reasons for `.nxcd` handling. Dump files are untrusted
/// input: every parse failure maps to exactly one of these variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NxcdError {
    /// Magic bytes are not `NXCD`.
    BadMagic,
    /// Container version is not supported by this decoder.
    UnsupportedVersion,
    /// Input is shorter than the declared layout requires.
    Truncated,
    /// Declared total length does not match the input length.
    LengthMismatch,
    /// Section table is not strictly ascending by section kind.
    SectionOrder,
    /// Section kind byte is outside the known set.
    UnknownSection,
    /// A section exceeds its per-kind size bound or the container bound.
    OversizeSection,
    /// A required section (header/frames/maps) is missing.
    MissingRequired,
    /// Payloads are not canonically packed (offset gaps or overlaps).
    NonCanonicalLayout,
    /// JSON section payload failed to encode.
    JsonEncode,
    /// JSON section payload failed to decode.
    JsonDecode,
    /// zstd wrapper input/output exceeded the configured bound.
    ZstBound,
    /// zstd wrapper codec failure (corrupt stream).
    ZstCodec,
}

impl core::fmt::Display for NxcdError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let label = match self {
            Self::BadMagic => "bad magic",
            Self::UnsupportedVersion => "unsupported version",
            Self::Truncated => "truncated container",
            Self::LengthMismatch => "length mismatch",
            Self::SectionOrder => "section table out of order",
            Self::UnknownSection => "unknown section kind",
            Self::OversizeSection => "oversize section",
            Self::MissingRequired => "missing required section",
            Self::NonCanonicalLayout => "non-canonical payload layout",
            Self::JsonEncode => "json encode failure",
            Self::JsonDecode => "json decode failure",
            Self::ZstBound => "zstd size bound exceeded",
            Self::ZstCodec => "zstd codec failure",
        };
        write!(f, "{label}")
    }
}

impl std::error::Error for NxcdError {}
