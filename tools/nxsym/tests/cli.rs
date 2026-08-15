// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Process-boundary tests for the `nxsym` CLI: index a real binary
//! (this test executable), then resolve a known frame through the index file.
//! OWNERS: @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 3 integration tests
//! ADR: tasks/TASK-0048-crashdump-v2a-host-pipeline-nxsym-nx-crash.md

use object::{Object, ObjectSymbol};
use std::process::Command;

/// Known frame for symbolization proofs (address discovered via symtab).
#[inline(never)]
#[no_mangle]
pub extern "C" fn nxsym_cli_fixture_frame() -> u64 {
    42
}

fn fixture_pc(exe_bytes: &[u8]) -> u64 {
    let obj = object::File::parse(exe_bytes).expect("parse test executable");
    for sym in obj.symbols() {
        if let Ok(name) = sym.name() {
            if name.contains("nxsym_cli_fixture_frame") {
                return sym.address();
            }
        }
    }
    panic!("fixture symbol not found in test executable");
}

#[test]
fn test_cli_index_and_addr2line_roundtrip() {
    let _ = nxsym_cli_fixture_frame();
    let exe = std::env::current_exe().expect("current exe");
    let bytes = std::fs::read(&exe).expect("read exe");
    let pc = fixture_pc(&bytes);
    let stem = exe.file_stem().expect("stem").to_string_lossy().into_owned();
    let (build_id, _) = nxsym::build_id_for_elf(&stem, &bytes).expect("build id");

    let dir = tempfile::tempdir().expect("tempdir");
    let sym_path = dir.path().join("symbols.nxsym");

    let out = Command::new(env!("CARGO_BIN_EXE_nxsym"))
        .arg("index")
        .arg(&exe)
        .arg("-o")
        .arg(&sym_path)
        .output()
        .expect("run nxsym index");
    assert!(out.status.success(), "index failed: {}", String::from_utf8_lossy(&out.stderr));
    assert!(sym_path.is_file());

    let out = Command::new(env!("CARGO_BIN_EXE_nxsym"))
        .arg("addr2line")
        .arg("--sym")
        .arg(&sym_path)
        .arg("--addr")
        .arg(format!("0x{pc:x}"))
        .arg("--build-id")
        .arg(&build_id)
        .output()
        .expect("run nxsym addr2line");
    assert!(out.status.success(), "addr2line failed: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("nxsym_cli_fixture_frame"), "unexpected output: {stdout}");
}

#[test]
fn test_cli_reject_bad_index_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sym_path = dir.path().join("symbols.nxsym");
    std::fs::write(&sym_path, b"garbage").expect("write garbage");
    let out = Command::new(env!("CARGO_BIN_EXE_nxsym"))
        .arg("addr2line")
        .arg("--sym")
        .arg(&sym_path)
        .arg("--addr")
        .arg("0x1000")
        .output()
        .expect("run nxsym addr2line");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("parse index"), "unexpected stderr: {stderr}");
}

#[test]
fn test_cli_reject_non_elf_index_input() {
    let dir = tempfile::tempdir().expect("tempdir");
    let not_elf = dir.path().join("input.bin");
    std::fs::write(&not_elf, b"not an elf").expect("write input");
    let out = Command::new(env!("CARGO_BIN_EXE_nxsym"))
        .arg("index")
        .arg(&not_elf)
        .arg("-o")
        .arg(dir.path().join("symbols.nxsym"))
        .output()
        .expect("run nxsym index");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("elf parse failure"), "unexpected stderr: {stderr}");
}
