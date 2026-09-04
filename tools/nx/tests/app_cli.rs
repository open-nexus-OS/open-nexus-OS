// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Process-boundary test for `nx app compile` (TASK-0321 P5): a
//! REAL app project from `userspace/apps` compiles to a non-empty payload
//! and the registry sidecar carries the manifest's label/icon/type; a
//! non-ui-program project is rejected.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 1 integration test
//! ADR: docs/adr/0060-verified-system-volume-bundlemgrd-verifier-init-spawner.md

use std::path::Path;
use std::process::Command;

#[test]
fn compiles_a_real_app_and_writes_the_sidecar() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("payload.elf");
    let meta = dir.path().join("meta/app.properties");
    let output = Command::new(env!("CARGO_BIN_EXE_nx"))
        .args([
            "app",
            "compile",
            "--app",
            repo.join("userspace/apps/calculator").to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--meta",
            meta.to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("nx runs");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stdout));
    let payload = std::fs::read(&out).expect("payload");
    assert!(!payload.is_empty());
    let props = std::fs::read_to_string(&meta).expect("sidecar");
    assert!(props.contains("label=Calculator\n"), "{props}");
    assert!(props.contains("bundle_type=app\n"), "{props}");
    assert!(props.contains("payload_kind=ui-program\n"), "{props}");
    // Determinism: the same project compiles to the same bytes.
    let again = Command::new(env!("CARGO_BIN_EXE_nx"))
        .args([
            "app",
            "compile",
            "--app",
            repo.join("userspace/apps/calculator").to_str().unwrap(),
            "--out",
            dir.path().join("payload2.elf").to_str().unwrap(),
        ])
        .output()
        .expect("nx runs");
    assert!(again.status.success());
    assert_eq!(std::fs::read(dir.path().join("payload2.elf")).unwrap(), payload);
    // Not a ui-program → rejected (no half bundle).
    let bad = Command::new(env!("CARGO_BIN_EXE_nx"))
        .args([
            "app",
            "compile",
            "--app",
            repo.join("userspace/apps/window-kit").to_str().unwrap(),
            "--out",
            dir.path().join("x").to_str().unwrap(),
        ])
        .output()
        .expect("nx runs");
    assert!(!bad.status.success());
}
