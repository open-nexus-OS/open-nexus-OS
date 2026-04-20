// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! Minimal in-test tempdir for `nexus-evidence` integration tests
//! (P5-03+). Living under `tests/tempdir/mod.rs` makes it usable
//! from any test file via `mod tempdir; use tempdir::Tempdir;`.
//!
//! Cleanup is best-effort on drop; failures are swallowed so a
//! still-mounted file (e.g. an open writer in a buggy test) does
//! not mask the real assertion failure.

use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicUsize, Ordering};

static SEQ: AtomicUsize = AtomicUsize::new(0);

pub struct Tempdir {
    path: PathBuf,
}

impl Tempdir {
    pub fn new(label: &str) -> Self {
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "nexus-evidence-test-{}-{}-{}",
            label,
            process::id(),
            n
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Tempdir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
