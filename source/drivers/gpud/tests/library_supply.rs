// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: gpud holds no pipeline library of its own (TASK-0215 5c-3 P3, ADR-0072 §6, RFC-0101
//! §7b): the compositor's GPU code reaches gpud from windowd, which opens its context with it
//! over IPC — on the board and on QEMU alike. This gate fails when gpud's code embeds a library
//! again; the board rungs' fixture (`backend/gpu/render_library.rs`, a test client's library for
//! the boot rungs, which run before any client) is the one exception.
//! OWNERS: @gpu @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: this file (`cargo test -p gpud --test library_supply`)

use std::fs;
use std::path::{Path, PathBuf};

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn test_reject_a_pipeline_library_embedded_in_gpud() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_sources(&src, &mut files);
    let mut embedding: Vec<String> = files
        .iter()
        .filter(|f| fs::read_to_string(f).is_ok_and(|text| text.contains(".nxgp\"")))
        .map(|f| f.strip_prefix(&src).unwrap_or(f).display().to_string())
        .collect();
    embedding.sort();
    assert_eq!(
        embedding,
        ["backend/gpu/render_library.rs"],
        "a pipeline library is the client's (windowd supplies it over IPC); gpud embeds none \
         beyond the board rungs' fixture"
    );
}
