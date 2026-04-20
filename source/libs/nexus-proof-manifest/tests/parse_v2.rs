// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0023B Cut P5-00 — schema v2 (`[include]`-based per-phase
//! split) parser acceptance + reject suite.
//!
//! Each test materializes a synthetic v2 manifest tree under a per-test
//! tempdir, then invokes `nexus_proof_manifest::parse_path` and asserts a
//! specific outcome (success or a stable [`ParseError`] variant). The
//! tests are file-system based on purpose: v2 only makes sense across
//! multiple files on disk, and the `[include]` glob expansion + duplicate
//! detection across files is the entire point of this cut.
//!
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable (variant set is append-only across cuts)
//! TEST_COVERAGE: 8 tests (1 accept + 7 reject) per the P5-00 plan
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use nexus_proof_manifest::{parse_path, ParseError};

/// Per-test temp directory (deleted on Drop). Counter avoids collisions
/// when cargo runs tests concurrently in the same process tree.
struct Dir {
    path: PathBuf,
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

impl Dir {
    fn new(label: &str) -> Self {
        let mut p = std::env::temp_dir();
        let nonce = format!(
            "nxs-parse-v2-{}-{}-{}",
            label,
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        );
        p.push(nonce);
        fs::create_dir_all(&p).expect("mkdir tempdir");
        Self { path: p }
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn write(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("mkdir parent");
    }
    fs::write(path, body).expect("write file");
}

/// Materialize a minimal-but-valid v2 manifest tree (1 phase, 1 marker,
/// 1 profile). Returns the root manifest path. Tests that need to inject
/// a single rejection class build on top of this fixture.
fn make_minimal_tree(dir: &Path) -> PathBuf {
    let root = dir.join("manifest.toml");
    write(
        &root,
        r#"[meta]
schema_version = "2"
default_profile = "full"

[include]
phases = "phases.toml"
markers = "markers/*.toml"
profiles = "profiles/*.toml"
"#,
    );
    write(
        &dir.join("phases.toml"),
        r#"[phase.bringup]
order = 1

[phase.end]
order = 2
"#,
    );
    write(
        &dir.join("markers/bringup.toml"),
        r#"[marker."hello"]
phase = "bringup"
"#,
    );
    write(
        &dir.join("profiles/harness.toml"),
        r#"[profile.full]
runner = "scripts/qemu-test.sh"
env = {}
"#,
    );
    root
}

// ---------------------------------------------------------------------------
// Test 1: accept
// ---------------------------------------------------------------------------

#[test]
fn v2_split_layout_parses_and_records_source_files() {
    let dir = Dir::new("accept");
    let root = make_minimal_tree(&dir.path);
    let m = parse_path(&root).expect("v2 minimal tree must parse");

    assert_eq!(m.meta.schema_version, "2");
    assert_eq!(m.meta.default_profile, "full");
    assert_eq!(m.phases.len(), 2);
    assert!(m.phases.contains_key("bringup"));
    assert_eq!(m.markers.len(), 1);
    assert_eq!(m.markers[0].literal, "hello");
    assert_eq!(m.profiles.len(), 1);
    assert!(m.profiles.contains_key("full"));

    // source_files: root first, then phases / markers / profiles.
    assert_eq!(m.source_files[0], root);
    assert!(
        m.source_files.len() >= 4,
        "expected root + 3 included files, got {:?}",
        m.source_files
    );
    assert!(m.source_files.iter().any(|p| p.ends_with("phases.toml")));
    assert!(m.source_files.iter().any(|p| p.ends_with("markers/bringup.toml")));
    assert!(m.source_files.iter().any(|p| p.ends_with("profiles/harness.toml")));
}

// ---------------------------------------------------------------------------
// Test 2: duplicate marker across files
// ---------------------------------------------------------------------------

#[test]
fn reject_duplicate_marker_across_files() {
    let dir = Dir::new("dup-marker");
    let root = make_minimal_tree(&dir.path);
    // Add a second markers/*.toml that re-declares the literal `"hello"`.
    write(
        &dir.path.join("markers/end.toml"),
        r#"[marker."hello"]
phase = "end"
"#,
    );
    let err = parse_path(&root).expect_err("duplicate literal across files must reject");
    match err {
        ParseError::DuplicateMarkerAcrossFiles { marker, first, second } => {
            assert_eq!(marker, "hello");
            // Glob expansion is lexicographically sorted: bringup.toml < end.toml.
            assert!(first.ends_with("markers/bringup.toml"), "first={first}");
            assert!(second.ends_with("markers/end.toml"), "second={second}");
        }
        other => panic!("expected DuplicateMarkerAcrossFiles, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Test 3: duplicate phase across files
// ---------------------------------------------------------------------------

#[test]
fn reject_duplicate_phase_across_files() {
    let dir = Dir::new("dup-phase");
    let root = dir.path.join("manifest.toml");
    write(
        &root,
        r#"[meta]
schema_version = "2"
default_profile = "full"

[include]
phases = "phases-*.toml"
markers = "markers/*.toml"
profiles = "profiles/*.toml"
"#,
    );
    write(
        &dir.path.join("phases-a.toml"),
        r#"[phase.bringup]
order = 1
"#,
    );
    write(
        &dir.path.join("phases-b.toml"),
        r#"[phase.bringup]
order = 2
"#,
    );
    write(
        &dir.path.join("markers/bringup.toml"),
        r#"[marker."hi"]
phase = "bringup"
"#,
    );
    write(
        &dir.path.join("profiles/harness.toml"),
        r#"[profile.full]
runner = "scripts/qemu-test.sh"
env = {}
"#,
    );
    let err = parse_path(&root).expect_err("duplicate phase across files must reject");
    assert!(
        matches!(err, ParseError::DuplicatePhaseAcrossFiles { ref phase, .. } if phase == "bringup"),
        "expected DuplicatePhaseAcrossFiles(\"bringup\"), got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 4: duplicate profile across files
// ---------------------------------------------------------------------------

#[test]
fn reject_duplicate_profile_across_files() {
    let dir = Dir::new("dup-profile");
    let root = make_minimal_tree(&dir.path);
    // Add a second profiles/*.toml that re-declares `[profile.full]`.
    write(
        &dir.path.join("profiles/runtime.toml"),
        r#"[profile.full]
runtime_only = true
phases = ["bringup", "end"]
"#,
    );
    let err = parse_path(&root).expect_err("duplicate profile across files must reject");
    assert!(
        matches!(err, ParseError::DuplicateProfileAcrossFiles { ref profile, .. } if profile == "full"),
        "expected DuplicateProfileAcrossFiles(\"full\"), got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 5: empty glob match
// ---------------------------------------------------------------------------

#[test]
fn reject_empty_glob() {
    let dir = Dir::new("empty-glob");
    let root = dir.path.join("manifest.toml");
    write(
        &root,
        r#"[meta]
schema_version = "2"
default_profile = "full"

[include]
phases = "phases.toml"
markers = "markers/*.toml"
profiles = "profiles/*.toml"
"#,
    );
    write(
        &dir.path.join("phases.toml"),
        r#"[phase.bringup]
order = 1
"#,
    );
    // Intentionally do NOT create markers/*.toml so the glob matches zero files.
    write(
        &dir.path.join("profiles/harness.toml"),
        r#"[profile.full]
runner = "scripts/qemu-test.sh"
env = {}
"#,
    );
    let err = parse_path(&root).expect_err("empty markers glob must reject");
    match err {
        ParseError::IncludeGlobEmpty { category, pattern } => {
            assert_eq!(category, "markers");
            assert_eq!(pattern, "markers/*.toml");
        }
        other => panic!("expected IncludeGlobEmpty {{ category=markers }}, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Test 6: nested include
// ---------------------------------------------------------------------------

#[test]
fn reject_nested_include_in_included_file() {
    let dir = Dir::new("nested");
    let root = make_minimal_tree(&dir.path);
    // Overwrite an included file so it itself carries [meta] + [include]
    // (a nested v2 root).
    write(
        &dir.path.join("markers/bringup.toml"),
        r#"[meta]
schema_version = "2"
default_profile = "full"

[include]
markers = "*.toml"
"#,
    );
    let err = parse_path(&root).expect_err("nested [include] must reject");
    assert!(
        matches!(err, ParseError::NestedInclude(ref p) if p.ends_with("markers/bringup.toml")),
        "expected NestedInclude(...markers/bringup.toml), got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 7: mixed schema in root (v2 root inlines a marker)
// ---------------------------------------------------------------------------

#[test]
fn reject_mixed_schema_in_root() {
    let dir = Dir::new("mixed");
    let root = dir.path.join("manifest.toml");
    write(
        &root,
        r#"[meta]
schema_version = "2"
default_profile = "full"

[include]
phases = "phases.toml"
markers = "markers/*.toml"
profiles = "profiles/*.toml"

[marker."inline-bad"]
phase = "bringup"
"#,
    );
    write(
        &dir.path.join("phases.toml"),
        r#"[phase.bringup]
order = 1
"#,
    );
    write(
        &dir.path.join("markers/bringup.toml"),
        r#"[marker."hi"]
phase = "bringup"
"#,
    );
    write(
        &dir.path.join("profiles/harness.toml"),
        r#"[profile.full]
runner = "scripts/qemu-test.sh"
env = {}
"#,
    );
    let err = parse_path(&root).expect_err("v2 root with inline marker must reject");
    assert!(
        matches!(err, ParseError::MixedSchemaInRoot(ref k) if k == "marker"),
        "expected MixedSchemaInRoot(\"marker\"), got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 8: lexicographic glob ordering
// ---------------------------------------------------------------------------

#[test]
fn glob_expansion_is_lexicographically_sorted() {
    // We can't easily mock dirent ordering portably, but we CAN assert
    // that source_files reflects sorted order regardless of which file
    // we wrote first. Write `c.toml` then `a.toml` then `b.toml`.
    let dir = Dir::new("sorted");
    let root = dir.path.join("manifest.toml");
    write(
        &root,
        r#"[meta]
schema_version = "2"
default_profile = "full"

[include]
phases = "phases.toml"
markers = "markers/*.toml"
profiles = "profiles/*.toml"
"#,
    );
    write(
        &dir.path.join("phases.toml"),
        r#"[phase.bringup]
order = 1
"#,
    );
    write(
        &dir.path.join("markers/c.toml"),
        r#"[marker."c"]
phase = "bringup"
"#,
    );
    write(
        &dir.path.join("markers/a.toml"),
        r#"[marker."a"]
phase = "bringup"
"#,
    );
    write(
        &dir.path.join("markers/b.toml"),
        r#"[marker."b"]
phase = "bringup"
"#,
    );
    write(
        &dir.path.join("profiles/harness.toml"),
        r#"[profile.full]
runner = "scripts/qemu-test.sh"
env = {}
"#,
    );
    let m = parse_path(&root).expect("must parse");
    // Markers preserve declaration order *within a file*, and files are
    // visited in lexicographic order: a → b → c.
    let lits: Vec<&str> = m.markers.iter().map(|mk| mk.literal.as_str()).collect();
    assert_eq!(lits, vec!["a", "b", "c"], "glob expansion must be lexicographically sorted");

    // source_files entries for the markers directory must also be sorted.
    let marker_sources: Vec<String> = m
        .source_files
        .iter()
        .filter(|p| p.to_string_lossy().contains("markers/"))
        .map(|p| p.display().to_string())
        .collect();
    let mut sorted = marker_sources.clone();
    sorted.sort();
    assert_eq!(marker_sources, sorted, "source_files must be lex-sorted within a glob");
}
