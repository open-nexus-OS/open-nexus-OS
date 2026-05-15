// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Init-lite input service startup contract tests
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal test
//! TEST_COVERAGE: `cargo test -p nx --test init_lite_input_service_startup`
//! ADR: docs/adr/0017-service-architecture.md

use std::fs;
use std::path::{Path, PathBuf};

const INPUT_SERVICES: [&str; 3] = ["hidrawd", "touchd", "inputd"];

#[test]
fn input_services_are_default_init_lite_candidates() {
    let build_rs = read_repo_file("source/apps/init-lite/build.rs");

    for service in INPUT_SERVICES {
        assert!(
            build_rs.contains(&format!("\"{service}\"")),
            "init-lite default candidates must include input service `{service}`"
        );
    }
}

#[test]
fn input_services_are_in_default_qemu_payload_list() {
    let qemu_test = read_repo_file("scripts/qemu-test.sh");
    let run_qemu = read_repo_file("scripts/run-qemu-rv64.sh");
    // Phase 4b: auto-discovery via scripts/discover-services.sh and cargo metadata.
    // The Makefile no longer hardcodes service lists; services declare themselves
    // via [package.metadata.nexus-service] in their Cargo.toml.
    for service in INPUT_SERVICES {
        assert_service_list_contains(&qemu_test, service, "scripts/qemu-test.sh");
        assert_service_list_contains(&run_qemu, service, "scripts/run-qemu-rv64.sh");
        // Verify the service has nexus-service metadata (auto-discovered)
        let cargo_toml = read_repo_file(format!("source/services/{service}/Cargo.toml"));
        assert!(
            cargo_toml.contains("[package.metadata.nexus-service]"),
            "`source/services/{service}/Cargo.toml` must declare [package.metadata.nexus-service] for auto-discovery"
        );
    }
    assert!(
        !qemu_test.contains("INPUT_V1_0B_SERVICE_STARTUP"),
        "`scripts/qemu-test.sh` must not hide input service startup behind task-specific profile gating"
    );
    assert!(
        qemu_test.contains("RUN_PHASE\" == \"input-startup\"")
            && qemu_test.contains("\"inputd: os service payload ready\""),
        "`scripts/qemu-test.sh` must keep a focused input-startup proof ladder for real service payload readiness"
    );
    assert!(
        run_qemu.contains("hidrawd|touchd|inputd"),
        "`scripts/run-qemu-rv64.sh` must keep bounded stack support for explicit input service lists"
    );
}

#[test]
fn input_services_use_bounded_os_stack_pages() {
    // Phase 4b: stack_pages is declared in Cargo.toml metadata,
    // resolved by scripts/discover-services.sh --env-vars.
    for service in INPUT_SERVICES {
        let cargo_toml = read_repo_file(format!("source/services/{service}/Cargo.toml"));
        assert!(
            cargo_toml.contains("stack_pages = 1"),
            "`source/services/{service}/Cargo.toml` must have stack_pages = 1"
        );
    }

    let discover = read_repo_file("scripts/discover-services.sh");
    assert!(
        discover.contains("stack_pages"),
        "`scripts/discover-services.sh` must resolve stack_pages from cargo metadata"
    );
}

#[test]
fn qemu_marker_ladder_requires_input_service_startup() {
    let qemu_test = read_repo_file("scripts/qemu-test.sh");
    let bringup_manifest =
        read_repo_file("source/apps/selftest-client/proof-manifest/markers/bringup.toml");

    for service in INPUT_SERVICES {
        for marker in [
            format!("init: start {service}"),
            format!("init: up {service}"),
            format!("{service}: os service payload ready"),
        ] {
            assert!(
                qemu_test.contains(&marker),
                "`scripts/qemu-test.sh` expected sequence must require `{marker}`"
            );
            assert!(
                bringup_manifest.contains(&format!("[marker.\"{marker}\"]")),
                "proof-manifest bringup markers must declare `{marker}`"
            );
        }
    }
}

fn assert_service_list_contains(haystack: &str, service: &str, source: &str) {
    assert!(
        haystack.contains(service),
        "{source} must include input service `{service}` in its init-lite service payload list"
    );
}

fn read_repo_file(relative: impl AsRef<Path>) -> String {
    let path = repo_root().join(relative);
    fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
