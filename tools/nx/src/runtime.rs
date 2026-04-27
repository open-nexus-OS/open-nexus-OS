// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Runtime environment and repository path resolution for `nx`.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by `nx` command tests.
//! ADR: docs/adr/0021-structured-data-formats-json-vs-capnp.md

use crate::error::{ExitClass, NxError};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub(crate) struct RuntimeConfig {
    pub(crate) repo_root: PathBuf,
    pub(crate) postflight_dir: PathBuf,
    pub(crate) dsl_backend: Option<PathBuf>,
}

impl RuntimeConfig {
    pub(crate) fn from_env() -> Result<Self, NxError> {
        let repo_root = std::env::current_dir().map_err(|e| {
            NxError::new(ExitClass::Internal, format!("failed to resolve cwd: {e}"))
        })?;
        Ok(Self {
            postflight_dir: repo_root.join("tools"),
            dsl_backend: std::env::var_os("NX_DSL_BACKEND").map(PathBuf::from),
            repo_root,
        })
    }
}
