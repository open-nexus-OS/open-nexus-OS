// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Process-boundary tests for `nx crash ls/show/export/purge/grep`
//! over fixture dump directories (.nxcd, .nxcd.zst, legacy .nmd), including
//! symbolized `show --sym` against a real nxsym index of this test binary.
//! OWNERS: @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 8 integration tests
//! ADR: tasks/TASK-0048-crashdump-v2a-host-pipeline-nxsym-nx-crash.md

use object::{Object, ObjectSymbol};
use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};

/// Known frame for the `show --sym` proof.
#[inline(never)]
#[no_mangle]
pub extern "C" fn nx_crash_cli_fixture_frame() -> u64 {
    7
}

fn run_nx(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nx"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("nx process must run")
}

fn stdout_json(output: &Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout).expect("stdout must be valid json")
}

fn fixture_frame(ts: u64, pid: u32, name: &str) -> crash::MinidumpFrame {
    crash::MinidumpFrame {
        timestamp_nsec: ts,
        pid,
        code: 42,
        name: String::from(name),
        build_id: crash::deterministic_build_id(name),
        pcs: vec![0x1000, 0x2000],
        stack_preview: vec![0xAA; 16],
        code_preview: vec![0xCC; 8],
    }
}

/// Writes one dump in each supported format into `dir`.
fn write_fixture_dir(dir: &Path) {
    let nmd = fixture_frame(100, 1, "demo.alpha").encode().expect("encode nmd");
    std::fs::write(dir.join("100.1.demo.alpha.nmd"), nmd).expect("write nmd");

    let container = nxcd::from_minidump(&fixture_frame(200, 2, "demo.beta")).expect("convert");
    let bytes = container.encode().expect("encode nxcd");
    std::fs::write(dir.join("200.2.demo.beta.nxcd"), &bytes).expect("write nxcd");

    let container = nxcd::from_minidump(&fixture_frame(300, 3, "demo.gamma")).expect("convert");
    let z = nxcd::compress_nxcd(&container.encode().expect("encode")).expect("compress");
    std::fs::write(dir.join("300.3.demo.gamma.nxcd.zst"), z).expect("write nxcd.zst");
}

#[test]
fn test_crash_ls_lists_all_formats() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_fixture_dir(dir.path());
    let out = run_nx(&["crash", "ls", "--dir", ".", "--json"], dir.path());
    assert_eq!(out.status.code(), Some(0));
    let json = stdout_json(&out);
    let dumps = json["data"]["dumps"].as_array().expect("dumps array");
    assert_eq!(dumps.len(), 3);
    assert!(dumps.iter().all(|d| d["valid"] == true));
    let formats: Vec<&str> = dumps.iter().map(|d| d["format"].as_str().expect("format")).collect();
    assert_eq!(formats, vec!["nmd", "nxcd", "nxcd.zst"]);
    assert_eq!(dumps[1]["pid"], 2);
    assert_eq!(dumps[1]["name"], "demo.beta");
}

#[test]
fn test_crash_ls_flags_corrupt_dump_as_invalid() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_fixture_dir(dir.path());
    std::fs::write(dir.path().join("999.9.broken.nxcd"), b"garbage").expect("write corrupt");
    let out = run_nx(&["crash", "ls", "--dir", ".", "--json"], dir.path());
    assert_eq!(out.status.code(), Some(0));
    let json = stdout_json(&out);
    let dumps = json["data"]["dumps"].as_array().expect("dumps array");
    assert_eq!(dumps.len(), 4);
    let broken = dumps.iter().find(|d| d["id"] == "999.9.broken.nxcd").expect("broken row");
    assert_eq!(broken["valid"], false);
}

#[test]
fn test_crash_show_reads_compressed_dump() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_fixture_dir(dir.path());
    let out = run_nx(&["crash", "show", "300.3.demo.gamma.nxcd.zst", "--json"], dir.path());
    assert_eq!(out.status.code(), Some(0));
    let json = stdout_json(&out);
    assert_eq!(json["data"]["header"]["pid"], 3);
    assert_eq!(json["data"]["header"]["name"], "demo.gamma");
    assert_eq!(json["data"]["frames"].as_array().expect("frames").len(), 2);
    assert_eq!(json["data"]["symbolized"], false);
}

#[test]
fn test_crash_show_symbolizes_with_nxsym_index() {
    let _ = nx_crash_cli_fixture_frame();
    let dir = tempfile::tempdir().expect("tempdir");
    let exe = std::env::current_exe().expect("current exe");
    let bytes = std::fs::read(&exe).expect("read exe");
    let obj = object::File::parse(&*bytes).expect("parse exe");
    let mut pc = None;
    for sym in obj.symbols() {
        if let Ok(name) = sym.name() {
            if name.contains("nx_crash_cli_fixture_frame") {
                pc = Some(sym.address());
                break;
            }
        }
    }
    let pc = pc.expect("fixture symbol address");

    // Dump whose frame build_id matches the indexed binary's build id.
    let index = nxsym::build_index(std::slice::from_ref(&exe)).expect("index");
    let build_id = index.binaries[0].build_id.clone();
    let mut frame = fixture_frame(400, 4, "demo.sym");
    frame.build_id = build_id;
    frame.pcs = vec![pc];
    let container = nxcd::from_minidump(&frame).expect("convert");
    std::fs::write(dir.path().join("400.4.demo.sym.nxcd"), container.encode().expect("encode"))
        .expect("write dump");
    std::fs::write(dir.path().join("symbols.nxsym"), nxsym::write_index(&index).expect("cbor"))
        .expect("write index");

    let out = run_nx(
        &["crash", "show", "400.4.demo.sym.nxcd", "--sym", "symbols.nxsym", "--json"],
        dir.path(),
    );
    assert_eq!(out.status.code(), Some(0));
    let json = stdout_json(&out);
    assert_eq!(json["data"]["symbolized"], true);
    let function = json["data"]["frames"][0]["function"].as_str().expect("function");
    assert!(function.contains("nx_crash_cli_fixture_frame"), "got: {function}");
}

#[test]
fn test_crash_export_converts_nmd_to_canonical_zst() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_fixture_dir(dir.path());
    let out = run_nx(
        &["crash", "export", "100.1.demo.alpha.nmd", "-o", "exported.nxcd.zst", "--json"],
        dir.path(),
    );
    assert_eq!(out.status.code(), Some(0));
    let json = stdout_json(&out);
    assert_eq!(json["data"]["compressed"], true);
    // Exported artifact loads back through the canonical path.
    let out = run_nx(&["crash", "show", "exported.nxcd.zst", "--json"], dir.path());
    assert_eq!(out.status.code(), Some(0));
    let json = stdout_json(&out);
    assert_eq!(json["data"]["header"]["name"], "demo.alpha");
    assert_eq!(json["data"]["header"]["build_id"], crash::deterministic_build_id("demo.alpha"));
}

#[test]
fn test_crash_purge_applies_count_budget_keeping_newest() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_fixture_dir(dir.path());
    let out = run_nx(&["crash", "purge", "--dir", ".", "--max-count", "1", "--json"], dir.path());
    assert_eq!(out.status.code(), Some(0));
    let json = stdout_json(&out);
    let deleted = json["data"]["deleted"].as_array().expect("deleted");
    assert_eq!(deleted.len(), 2);
    // Newest (timestamp 300) survives.
    assert!(dir.path().join("300.3.demo.gamma.nxcd.zst").is_file());
    assert!(!dir.path().join("100.1.demo.alpha.nmd").exists());
    assert!(!dir.path().join("200.2.demo.beta.nxcd").exists());
}

#[test]
fn test_crash_grep_matches_and_misses() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_fixture_dir(dir.path());
    let out = run_nx(&["crash", "grep", "demo.beta", "--dir", ".", "--json"], dir.path());
    assert_eq!(out.status.code(), Some(0));
    let json = stdout_json(&out);
    let matches = json["data"]["matches"].as_array().expect("matches");
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0], "200.2.demo.beta.nxcd");

    let out = run_nx(&["crash", "grep", "no-such-needle", "--dir", ".", "--json"], dir.path());
    let json = stdout_json(&out);
    assert_eq!(json["data"]["matches"].as_array().expect("matches").len(), 0);
}

#[test]
fn test_crash_reject_show_corrupt_and_missing_dir() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("bad.nxcd"), b"garbage").expect("write corrupt");
    let out = run_nx(&["crash", "show", "bad.nxcd", "--json"], dir.path());
    assert_eq!(out.status.code(), Some(3));
    let json = stdout_json(&out);
    assert_eq!(json["class"], "validation_reject");

    let out = run_nx(&["crash", "ls", "--dir", "does-not-exist", "--json"], dir.path());
    assert_eq!(out.status.code(), Some(3));
}
