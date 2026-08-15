// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Optional zstd wrapper for `.nxcd.zst` artifacts. Lives outside the
//! core container format: compression wraps the already-encoded container
//! bytes and decompression is bounded before any container parsing happens.
//! Host tools only (feature `zst`); never part of the OS graph.
//! OWNERS: @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Unit tests below; integration in `tests/crashdump_v2_host`
//! ADR: tasks/TASK-0048-crashdump-v2a-host-pipeline-nxsym-nx-crash.md

use crate::container::MAX_TOTAL_NXCD;
use crate::NxcdError;
use std::io::Read;

/// Compression level for `.nxcd.zst` (zstd default level, deterministic for
/// a fixed zstd version and input).
const LEVEL: i32 = 3;

/// Compress encoded `.nxcd` bytes into a `.nxcd.zst` payload.
pub fn compress_nxcd(nxcd_bytes: &[u8]) -> Result<Vec<u8>, NxcdError> {
    if nxcd_bytes.len() > MAX_TOTAL_NXCD {
        return Err(NxcdError::ZstBound);
    }
    zstd::stream::encode_all(nxcd_bytes, LEVEL).map_err(|_| NxcdError::ZstCodec)
}

/// Decompress an untrusted `.nxcd.zst` payload with a hard output bound.
/// Streams at most `MAX_TOTAL_NXCD + 1` bytes so a decompression bomb is
/// rejected without buffering its expansion.
pub fn decompress_nxcd(zst_bytes: &[u8]) -> Result<Vec<u8>, NxcdError> {
    if zst_bytes.len() > MAX_TOTAL_NXCD {
        return Err(NxcdError::ZstBound);
    }
    let decoder = zstd::stream::read::Decoder::new(zst_bytes).map_err(|_| NxcdError::ZstCodec)?;
    let mut limited = decoder.take(MAX_TOTAL_NXCD as u64 + 1);
    let mut out = Vec::new();
    limited.read_to_end(&mut out).map_err(|_| NxcdError::ZstCodec)?;
    if out.len() > MAX_TOTAL_NXCD {
        return Err(NxcdError::ZstBound);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::{NxcdContainer, SectionKind};

    fn sample_bytes() -> Vec<u8> {
        let mut c = NxcdContainer::new();
        c.insert(SectionKind::Header, b"{\"pid\":7}".to_vec()).expect("header");
        c.insert(SectionKind::Frames, b"{\"frames\":[]}".to_vec()).expect("frames");
        c.insert(SectionKind::Maps, b"{\"modules\":[]}".to_vec()).expect("maps");
        c.encode().expect("encode")
    }

    #[test]
    fn test_zst_roundtrip() {
        let bytes = sample_bytes();
        let z = compress_nxcd(&bytes).expect("compress");
        let back = decompress_nxcd(&z).expect("decompress");
        assert_eq!(back, bytes);
        assert!(NxcdContainer::decode(&back).is_ok());
    }

    #[test]
    fn test_reject_zst_garbage_stream() {
        assert_eq!(decompress_nxcd(&[0x11, 0x22, 0x33, 0x44]), Err(NxcdError::ZstCodec));
    }

    #[test]
    fn test_reject_zst_decompression_bomb() {
        // Compress a payload larger than the container bound; the bounded
        // reader must reject it instead of buffering the expansion.
        let big = vec![0u8; MAX_TOTAL_NXCD + 4096];
        let z = zstd::stream::encode_all(&big[..], LEVEL).expect("compress");
        assert_eq!(decompress_nxcd(&z), Err(NxcdError::ZstBound));
    }
}
