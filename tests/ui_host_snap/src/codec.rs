// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Snapshot error and BGRA/RGBA/hex codec helpers for host goldens.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 24 ui_host_snap integration tests
//! ADR: docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md

use std::error::Error;
use std::fmt;

use ui_renderer::BYTES_PER_PIXEL;

pub type SnapResult<T> = Result<T, SnapshotError>;

#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "snapshot errors must be handled"]
pub enum SnapshotError {
    FixturePathRejected,
    GoldenMismatch,
    GoldenUpdateDisabled,
    ImageDataInvalid,
    Codec,
    Io(String),
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FixturePathRejected => f.write_str("fixture_path_rejected"),
            Self::GoldenMismatch => f.write_str("golden_mismatch"),
            Self::GoldenUpdateDisabled => f.write_str("golden_update_disabled"),
            Self::ImageDataInvalid => f.write_str("image_data_invalid"),
            Self::Codec => f.write_str("codec_error"),
            Self::Io(kind) => write!(f, "io_error:{kind}"),
        }
    }
}

impl Error for SnapshotError {}

impl From<std::io::Error> for SnapshotError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.kind().to_string())
    }
}

pub fn normalize_hex(input: &str) -> String {
    input.chars().filter(|ch| !ch.is_whitespace()).collect()
}

pub fn hex_bytes(bytes: &[u8]) -> SnapResult<String> {
    let mut out = String::with_capacity(bytes.len().saturating_mul(2) + bytes.len() / 16);
    for chunk in bytes.chunks(16) {
        for byte in chunk {
            fmt::Write::write_fmt(&mut out, format_args!("{byte:02x}"))
                .map_err(|_| SnapshotError::Codec)?;
        }
        out.push('\n');
    }
    Ok(out)
}

pub fn bgra_to_rgba(width: u32, height: u32, bgra: &[u8]) -> SnapResult<Vec<u8>> {
    let expected = usize::try_from(
        u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|pixels| pixels.checked_mul(u64::from(BYTES_PER_PIXEL)))
            .ok_or(SnapshotError::ImageDataInvalid)?,
    )
    .map_err(|_| SnapshotError::ImageDataInvalid)?;
    if bgra.len() != expected {
        return Err(SnapshotError::ImageDataInvalid);
    }
    let mut rgba = Vec::with_capacity(expected);
    for pixel in bgra.chunks_exact(4) {
        rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
    }
    Ok(rgba)
}
