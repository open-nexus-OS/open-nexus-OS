// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The golden machinery (harvested 2026-11-04 from `tests/ui_host_snap`, retired with ADR-0094):
//! canonical BGRA hex compare, the `UPDATE_GOLDENS=1` gate, update paths held under the golden
//! root. The design-system goldens compare the painter's bytes with it.

use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

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

/// Bytes per BGRA8888 pixel.
pub const BYTES_PER_PIXEL: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoldenMode {
    CompareOnly,
    Update,
}

impl GoldenMode {
    #[must_use]
    pub fn from_env() -> Self {
        match std::env::var("UPDATE_GOLDENS") {
            Ok(value) if value == "1" => Self::Update,
            _ => Self::CompareOnly,
        }
    }
}

#[must_use]
pub fn golden_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("goldens")
}

pub fn resolve_under_root(root: &Path, relative: &Path) -> SnapResult<PathBuf> {
    if relative.is_absolute() {
        return Err(SnapshotError::FixturePathRejected);
    }
    let mut clean = PathBuf::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(SnapshotError::FixturePathRejected);
            }
        }
    }
    if clean.as_os_str().is_empty() {
        return Err(SnapshotError::FixturePathRejected);
    }
    Ok(root.join(clean))
}

pub fn compare_hex_golden(
    root: &Path,
    relative: &Path,
    actual: &str,
    mode: GoldenMode,
) -> SnapResult<()> {
    let path = resolve_under_root(root, relative)?;
    match fs::read_to_string(&path) {
        Ok(expected) if normalize_hex(&expected) == normalize_hex(actual) => Ok(()),
        Ok(_) => {
            if mode == GoldenMode::Update {
                fs::write(path, actual)?;
                Ok(())
            } else {
                Err(SnapshotError::GoldenMismatch)
            }
        }
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound && mode == GoldenMode::Update =>
        {
            fs::write(path, actual)?;
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(SnapshotError::GoldenMismatch)
        }
        Err(error) => Err(error.into()),
    }
}

pub fn update_hex_golden(
    root: &Path,
    relative: &Path,
    actual: &str,
    mode: GoldenMode,
) -> SnapResult<()> {
    if mode != GoldenMode::Update {
        return Err(SnapshotError::GoldenUpdateDisabled);
    }
    let path = resolve_under_root(root, relative)?;
    fs::write(path, actual)?;
    Ok(())
}
