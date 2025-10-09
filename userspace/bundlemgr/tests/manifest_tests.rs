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
