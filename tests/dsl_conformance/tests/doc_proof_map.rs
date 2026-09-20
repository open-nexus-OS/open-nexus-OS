// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! `docs/dev/dsl/testing.md` names, guarantee by guarantee, the test that
//! proves it. This checks the names resolve (TASK-0077B P4).
//!
//! A hand-maintained map of proofs rots the moment a test is renamed, and a map
//! that claims coverage which no longer exists is worse than no map: it stops
//! the next reader from looking. This repo has the failure mode on record —
//! DSL test fixtures that drifted from the code they mirrored and only said so
//! under `--no-fail-fast`. So the map is mechanically checked rather than
//! trusted, which is also what lets it name individual tests instead of
//! retreating to vague file-level pointers.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("repo root")
}

/// Every ``crate::file::test`` (or ``crate::file``) reference in the doc, as
/// written between backticks.
fn references(doc: &str) -> Vec<(String, String, Option<String>)> {
    let mut out = Vec::new();
    for raw in doc.split('`').skip(1).step_by(2) {
        let mut parts = raw.split("::");
        let (Some(krate), Some(file)) = (parts.next(), parts.next()) else { continue };
        if !krate.starts_with("dsl_") {
            continue;
        }
        let test = parts.next().map(str::to_owned);
        // Prose that happens to contain `::` has whitespace; a path does not.
        // The test is on the SHAPE of the whole reference, never on the
        // characters of a segment: filtering by character class silently
        // skipped exactly the malformed references this exists to catch — a
        // renamed test with one capital in it read as prose and passed.
        if raw.split_whitespace().count() != 1 || parts.next().is_some() {
            continue;
        }
        out.push((krate.to_owned(), file.to_owned(), test));
    }
    out
}

#[test]
fn every_proof_the_testing_doc_names_exists() {
    let root = repo_root();
    let doc_path = root.join("docs/dev/dsl/testing.md");
    let doc = std::fs::read_to_string(&doc_path).expect("testing.md is readable");
    let refs = references(&doc);
    assert!(refs.len() >= 8, "the guarantee map lost its references: {refs:?}");

    let mut missing = Vec::new();
    for (krate, file, test) in &refs {
        let path = root.join("tests").join(krate).join("tests").join(format!("{file}.rs"));
        let Ok(src) = std::fs::read_to_string(&path) else {
            missing.push(format!("{krate}::{file} — no such file ({})", path.display()));
            continue;
        };
        if let Some(test) = test {
            if !src.contains(&format!("fn {test}(")) {
                missing.push(format!("{krate}::{file}::{test} — not in {}", path.display()));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "docs/dev/dsl/testing.md names proofs that do not exist:\n  {}",
        missing.join("\n  ")
    );
}
