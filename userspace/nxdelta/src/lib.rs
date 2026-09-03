// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

//! CONTEXT: `.nxdelta` v1 (RFC-0090) — the boot-image delta stream carried
//! as the `.nxs` v2 `boot-image-delta` component kind (RFC-0089 §11). This
//! crate is the format SSOT both sides link: the device (`updated`) drives
//! the bounded streaming [`decode::Decoder`] (no_std, header-sized carry,
//! no per-record allocation), the host (`nx image ota --delta-from`) emits
//! via [`make`] (deterministic: rollsum index + greedy scan + COPY
//! coalescing — emitting twice yields identical bytes).
//!
//! Trust shape (normative context): the component's manifest `sha256`
//! covers the DELTA STREAM, so these bytes are signature-bound before the
//! decoder runs; the TARGET truth lives in the component's verbatim NXBD;
//! the base binds O(1) against the loader-verified ACTIVE NXBD digest.
//! OWNERS: @runtime @tools-team
//! STATUS: Functional
//! API_STABILITY: Stable (wire format; RFC-0090)
//! TEST_COVERAGE: unit tests below + tests/nxdelta_host + tests/updates_host
//! ADR: docs/rfcs/RFC-0090-nxdelta-boot-image-delta-stream-format.md

pub mod decode;
#[cfg(feature = "std")]
pub mod make;

/// Stream magic (header bytes 0..8).
pub const MAGIC: &[u8; 8] = b"NXDELTA1";
/// Format version this crate speaks.
pub const VERSION: u16 = 1;
/// ADD payloads are stored verbatim (v1's only algorithm; 1 = zstd RESERVED).
pub const ALGO_STORED: u8 = 0;
/// Fixed header length.
pub const HEADER_LEN: usize = 96;
/// Per-record payload/length cap (COPY len and ADD len alike).
pub const MAX_RECORD_LEN: u32 = 4 * 1024 * 1024;

/// Record tags.
pub const TAG_COPY: u8 = 0x01;
pub const TAG_ADD: u8 = 0x02;
pub const TAG_END: u8 = 0xFF;

/// Decoded stream header (RFC-0090, little-endian).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub algo: u8,
    pub base_size: u64,
    pub target_size: u64,
    pub base_sha256: [u8; 32],
    pub target_sha256: [u8; 32],
}

impl Header {
    /// Encodes the 96-byte header.
    #[must_use]
    pub fn encode(&self) -> [u8; HEADER_LEN] {
        let mut out = [0u8; HEADER_LEN];
        out[0..8].copy_from_slice(MAGIC);
        out[8..10].copy_from_slice(&VERSION.to_le_bytes());
        out[10] = self.algo;
        // bytes 11..16 reserved, zero.
        out[16..24].copy_from_slice(&self.base_size.to_le_bytes());
        out[24..32].copy_from_slice(&self.target_size.to_le_bytes());
        out[32..64].copy_from_slice(&self.base_sha256);
        out[64..96].copy_from_slice(&self.target_sha256);
        out
    }

    /// Bounded decode of exactly [`HEADER_LEN`] bytes. Unknown version or
    /// algorithm is a format error — never a silent fallback.
    pub fn decode(bytes: &[u8]) -> Result<Self, DeltaError> {
        if bytes.len() != HEADER_LEN || &bytes[0..8] != MAGIC {
            return Err(DeltaError::Format);
        }
        if u16::from_le_bytes([bytes[8], bytes[9]]) != VERSION {
            return Err(DeltaError::Format);
        }
        let algo = bytes[10];
        if algo != ALGO_STORED {
            return Err(DeltaError::Format);
        }
        if bytes[11..16].iter().any(|&b| b != 0) {
            return Err(DeltaError::Format);
        }
        let mut base_sha256 = [0u8; 32];
        base_sha256.copy_from_slice(&bytes[32..64]);
        let mut target_sha256 = [0u8; 32];
        target_sha256.copy_from_slice(&bytes[64..96]);
        Ok(Self {
            algo,
            base_size: u64::from_le_bytes(bytes[16..24].try_into().unwrap_or([0; 8])),
            target_size: u64::from_le_bytes(bytes[24..32].try_into().unwrap_or([0; 8])),
            base_sha256,
            target_sha256,
        })
    }
}

/// The one format-level error: every malformed shape rejects identically
/// (the caller maps it to the stable `delta-format` vocabulary).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeltaError {
    Format,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> Header {
        Header {
            algo: ALGO_STORED,
            base_size: 1000,
            target_size: 2000,
            base_sha256: [7u8; 32],
            target_sha256: [9u8; 32],
        }
    }

    #[test]
    fn header_roundtrip() {
        let bytes = header().encode();
        assert_eq!(Header::decode(&bytes), Ok(header()));
    }

    #[test]
    fn test_reject_header_mutations() {
        let good = header().encode();
        // Wrong magic, wrong version, unsupported algo, dirty reserved,
        // short and long inputs — every mutation is Format, never a panic.
        let mut m = good;
        m[0] ^= 1;
        assert!(Header::decode(&m).is_err(), "magic");
        let mut m = good;
        m[8] = 2;
        assert!(Header::decode(&m).is_err(), "version");
        let mut m = good;
        m[10] = 1;
        assert!(Header::decode(&m).is_err(), "algo zstd is reserved, not silently accepted");
        let mut m = good;
        m[12] = 1;
        assert!(Header::decode(&m).is_err(), "reserved");
        assert!(Header::decode(&good[..95]).is_err(), "short");
    }
}
