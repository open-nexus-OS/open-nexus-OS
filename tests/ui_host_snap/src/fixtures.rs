// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Common host snapshot fixtures for renderer integration tests.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 24 ui_host_snap integration tests
//! ADR: docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md

use std::fs;
use std::io::{Error, ErrorKind, Result as IoResult};
use std::path::{Component, Path, PathBuf};

use ui_renderer::{Damage, DamageRectCount, Frame, RenderError};

pub fn make_damage(frame: &Frame, max_count: u16) -> Result<Damage, RenderError> {
    Damage::for_frame(frame.width(), frame.height(), DamageRectCount::new(max_count)?)
}

pub fn artifact_root() -> IoResult<PathBuf> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join("ui_host_snap_artifacts")
        .join(std::process::id().to_string());
    fs::create_dir_all(&path)?;
    Ok(path)
}

pub fn temp_artifact_path(name: &str) -> IoResult<PathBuf> {
    let mut clean = PathBuf::new();
    for component in Path::new(name).components() {
        match component {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(Error::new(ErrorKind::InvalidInput, "artifact_path_rejected"));
            }
        }
    }
    if clean.as_os_str().is_empty() {
        return Err(Error::new(ErrorKind::InvalidInput, "artifact_path_rejected"));
    }
    Ok(artifact_root()?.join(clean))
}
