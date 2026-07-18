// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Guard for the curated App-SDK crate list (TASK-0081 C0):
//! `docs/dev/sdk/crates.toml` is the machine-readable SSOT — this crate's
//! tests pin that every `[sdk]` entry points at a real crate and that the
//! forbidden OS-internal crates never sneak onto the list.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: this crate IS the coverage (`tests/surface.rs`)

// reason: test harness — an unreadable repo root or SSOT file must panic loudly
// to fail the guard test, not be silently propagated.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::{Path, PathBuf};

/// Repo root (this crate lives at `tests/sdk_surface`).
#[must_use]
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("repo root")
}

/// Parses the `[sdk]` entries of `docs/dev/sdk/crates.toml` →
/// `(crate name, repo-relative path)`. Line-based on purpose (the file is
/// the SSOT; a shape change should break THIS parser loudly).
#[must_use]
pub fn sdk_entries() -> Vec<(String, String)> {
    let text = std::fs::read_to_string(repo_root().join("docs/dev/sdk/crates.toml"))
        .expect("crates.toml readable");
    let mut in_sdk = false;
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_sdk = line == "[sdk]";
            continue;
        }
        if !in_sdk || line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name, rest)) = line.split_once('=') else { continue };
        let name = name.trim().trim_matches('"').to_string();
        let path = rest
            .split("path =")
            .nth(1)
            .and_then(|p| p.split('"').nth(1))
            .unwrap_or_default()
            .to_string();
        out.push((name, path));
    }
    out
}
