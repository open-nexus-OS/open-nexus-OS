// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Unit tests for the host-side helpers of `nxs-pack`: argument
//! parsing, `system-set.toml` metadata, manifest decoding, the file size
//! bound and the SHA-256 digest. The archive format is not touched here.
//! OWNERS: @tools-team

use super::*;

fn args(list: &[&str]) -> Result<Args, Box<dyn std::error::Error>> {
    parse_args_from(list.iter().map(|s| s.to_string()))
}

fn error_text<T>(result: Result<T, Box<dyn std::error::Error>>) -> String {
    result.err().expect("the call must fail").to_string()
}

fn write_meta(dir: &tempfile::TempDir, toml: &str) -> PathBuf {
    let path = dir.path().join("system-set.toml");
    fs::write(&path, toml).expect("write system-set.toml");
    path
}

fn manifest_bytes(name: &str, semver: &str) -> Vec<u8> {
    let mut builder = Builder::new_default();
    let mut manifest = builder.init_root::<bundle_manifest::Builder<'_>>();
    manifest.set_name(name);
    manifest.set_semver(semver);
    serialize::write_message_to_words(&builder)
}

// --- parse_args -------------------------------------------------------------

#[test]
fn parse_args_reads_every_flag_in_any_order() {
    let parsed =
        args(&["--output", "out.nxs", "--key", "k.hex", "--input", "in", "--meta", "m.toml"])
            .expect("complete arguments parse");
    assert_eq!(parsed.input_dir, PathBuf::from("in"));
    assert_eq!(parsed.meta_path, PathBuf::from("m.toml"));
    assert_eq!(parsed.key_path, PathBuf::from("k.hex"));
    assert_eq!(parsed.output_path, PathBuf::from("out.nxs"));
}

#[test]
fn test_reject_missing_required_flag() {
    let full = ["--input", "in", "--meta", "m.toml", "--key", "k.hex", "--output", "out.nxs"];
    for (i, flag) in ["--input", "--meta", "--key", "--output"].into_iter().enumerate() {
        let mut without: Vec<&str> = full.to_vec();
        without.drain(i * 2..i * 2 + 2);
        assert_eq!(error_text(args(&without)), format!("missing {flag}"));
    }
}

#[test]
fn test_reject_flag_without_value() {
    assert_eq!(error_text(args(&["--input", "in", "--meta"])), "missing value for --meta");
}

#[test]
fn test_reject_unknown_argument() {
    assert_eq!(error_text(args(&["--input", "in", "--bogus"])), "unknown argument: --bogus");
}

// --- parse_meta / req_str / opt_u64 ----------------------------------------

#[test]
fn parse_meta_reads_version_and_timestamp() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path =
        write_meta(&dir, "system_version = \"  1.2.3  \"\ntimestamp_unix_ms = 1700000000000\n");
    let meta = parse_meta(&path).expect("valid metadata parses");
    assert_eq!(meta.system_version, "1.2.3", "the version is trimmed");
    assert_eq!(meta.timestamp_unix_ms, 1_700_000_000_000);
}

#[test]
fn parse_meta_defaults_a_missing_timestamp_to_zero() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_meta(&dir, "system_version = \"1.0.0\"\n");
    assert_eq!(parse_meta(&path).expect("parses").timestamp_unix_ms, 0);
}

#[test]
fn test_reject_meta_without_a_string_system_version() {
    let dir = tempfile::tempdir().expect("tempdir");
    for toml in ["timestamp_unix_ms = 1\n", "system_version = 1\n"] {
        let path = write_meta(&dir, toml);
        assert_eq!(
            error_text(parse_meta(&path)),
            "system-set.toml missing/invalid `system_version`",
            "{toml:?}"
        );
    }
}

#[test]
fn test_reject_meta_with_a_blank_system_version() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_meta(&dir, "system_version = \"   \"\n");
    assert_eq!(error_text(parse_meta(&path)), "system_version must not be empty");
}

#[test]
fn test_reject_meta_with_a_bad_timestamp() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_meta(&dir, "system_version = \"1.0.0\"\ntimestamp_unix_ms = -1\n");
    assert_eq!(error_text(parse_meta(&path)), "system-set.toml `timestamp_unix_ms` must be >= 0");
    let path = write_meta(&dir, "system_version = \"1.0.0\"\ntimestamp_unix_ms = \"soon\"\n");
    assert_eq!(
        error_text(parse_meta(&path)),
        "system-set.toml `timestamp_unix_ms` must be an integer"
    );
}

#[test]
fn test_reject_meta_that_is_not_toml_or_not_there() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_meta(&dir, "system_version = \n");
    assert!(parse_meta(&path).is_err(), "invalid TOML is an error");
    assert!(parse_meta(&dir.path().join("absent.toml")).is_err(), "a missing file is an error");
}

#[test]
fn req_str_and_opt_u64_read_a_table() {
    let table: toml::Table =
        toml::from_str("name = \"x\"\ncount = 7\n").expect("test table parses");
    assert_eq!(req_str(&table, "name").expect("present string"), "x");
    assert!(req_str(&table, "count").is_err(), "an integer is not a string");
    assert!(req_str(&table, "absent").is_err());
    assert_eq!(opt_u64(&table, "count").expect("present integer"), Some(7));
    assert_eq!(opt_u64(&table, "absent").expect("absent is fine"), None);
    assert!(opt_u64(&table, "name").is_err(), "a string is not an integer");
}

// --- parse_manifest ---------------------------------------------------------

#[test]
fn parse_manifest_reads_name_and_semver() {
    let (name, version) =
        parse_manifest(&manifest_bytes(" settings ", " 0.3.1 ")).expect("valid manifest");
    assert_eq!(name, "settings", "the name is trimmed");
    assert_eq!(version, "0.3.1", "the semver is trimmed");
}

#[test]
fn test_reject_manifest_with_a_blank_name_or_semver() {
    assert_eq!(
        error_text(parse_manifest(&manifest_bytes("  ", "1.0.0"))),
        "manifest name must not be empty"
    );
    assert_eq!(
        error_text(parse_manifest(&manifest_bytes("settings", ""))),
        "manifest semver must not be empty"
    );
}

#[test]
fn test_reject_manifest_that_is_not_capnp() {
    let message = error_text(parse_manifest(b"not a capnp message"));
    assert!(message.starts_with("manifest decode error:"), "{message}");
    assert!(parse_manifest(&[]).is_err(), "an empty manifest is an error");
}

// --- read_file_with_limit ---------------------------------------------------

#[test]
fn read_file_with_limit_accepts_a_file_at_the_limit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("payload.elf");
    fs::write(&path, [7u8; 16]).expect("write payload");
    assert_eq!(read_file_with_limit(&path, 16).expect("at the limit"), vec![7u8; 16]);
}

#[test]
fn test_reject_file_over_the_limit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("payload.elf");
    fs::write(&path, [7u8; 17]).expect("write payload");
    let message = error_text(read_file_with_limit(&path, 16));
    assert!(message.ends_with("too large: 17 bytes (max 16)"), "{message}");
    assert!(read_file_with_limit(&dir.path().join("absent"), 16).is_err());
}

// --- sha256 -----------------------------------------------------------------

#[test]
fn sha256_matches_the_standard_vectors() {
    // Known SHA-256 digests: "abc" is the FIPS 180-2 example, "" the empty input.
    assert_eq!(
        hex::encode(sha256(b"")),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        hex::encode(sha256(b"abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}
