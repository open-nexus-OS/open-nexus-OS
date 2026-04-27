// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Canonical golden path validation and update gating for snapshots.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 24 ui_host_snap integration tests
//! ADR: docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md

use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::codec::{normalize_hex, SnapResult, SnapshotError};

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
