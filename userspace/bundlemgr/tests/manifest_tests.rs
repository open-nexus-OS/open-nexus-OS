// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Integration tests for bundle manifest parsing and validation
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 3 manifest tests
//!
//! TEST_SCOPE:
//!   - Manifest parsing from TOML
//!   - Field validation and error handling
//!   - Version and capability parsing
//!   - Warning generation
//!
//! TEST_SCENARIOS:
//!   - test_parse_valid_manifest(): Parse valid manifest with all fields
//!   - test_missing_fields_error(): Handle missing required fields
//!   - test_invalid_caps_error(): Handle invalid capability specifications
//!
//! DEPENDENCIES:
//!   - bundlemgr::Manifest: Manifest parsing functionality
//!   - Test manifest files in tests/manifests/
//!   - std::fs: File system operations
//!
//! ADR: docs/adr/0009-bundle-manager-architecture.md

use std::fs;

use bundlemgr::{Error, Manifest};

fn manifest_path(name: &str) -> String {
    format!("{}/tests/manifests/{}", env!("CARGO_MANIFEST_DIR"), name)
}

#[test]
fn parse_valid_manifest() {
    let data = fs::read_to_string(manifest_path("valid_1.toml")).expect("read manifest");
    let manifest = Manifest::parse_str(&data).expect("manifest parsed");
    assert_eq!(manifest.name, "com.example.app");
    assert_eq!(manifest.version.to_string(), "1.2.3");
    assert_eq!(manifest.abilities, vec!["ui".to_string(), "storage".to_string()]);
    assert_eq!(manifest.capabilities, vec!["camera".to_string(), "network".to_string()]);
    assert_eq!(manifest.min_sdk.to_string(), "0.5.0");
    assert_eq!(manifest.warnings.len(), 1);
    assert!(manifest.warnings[0].contains("notes"));
}

#[test]
fn missing_fields_error() {
    let data =
        fs::read_to_string(manifest_path("invalid_missing_fields.toml")).expect("read manifest");
    let err = Manifest::parse_str(&data).expect_err("expected failure");
    assert_eq!(err, Error::MissingField("abilities"));
}

#[test]
fn invalid_caps_error() {
    let data = fs::read_to_string(manifest_path("invalid_caps.toml")).expect("read manifest");
    let err = Manifest::parse_str(&data).expect_err("expected failure");
    match err {
        Error::InvalidField { field, .. } => assert_eq!(field, "caps"),
        other => panic!("unexpected error: {other:?}"),
    }
}
