// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Manifest v2.1 `payloadKind` round-trip (TASK-0080D R2): a DSL
//! manifest (`payload_kind = "ui-program"`) packs `payload.nxir`, the
//! re-parsed manifest carries the kind; a default manifest keeps the
//! backward-compatible `elf` + `payload.elf`.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 2 tests

use std::path::PathBuf;
use std::process::Command;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nxb-pack-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn manifest_toml(payload_kind: Option<&str>) -> String {
    let mut toml = String::from(
        "name = \"demo.dslapp\"\nversion = \"0.1.0\"\nmin_sdk = \"0.1.0\"\n\
         abilities = [\"main\"]\ncaps = [\"nexus.permission.WINDOW\"]\n",
    );
    if let Some(kind) = payload_kind {
        toml.push_str(&format!("payload_kind = \"{kind}\"\n"));
    }
    toml
}

fn run_pack(dir: &PathBuf, toml: &str, payload: &[u8]) {
    let toml_path = dir.join("manifest.toml");
    let payload_path = dir.join("payload.bin");
    std::fs::write(&toml_path, toml).expect("write toml");
    std::fs::write(&payload_path, payload).expect("write payload");
    let out = dir.join("out");
    let status = Command::new(env!("CARGO_BIN_EXE_nxb-pack"))
        .arg("--toml")
        .arg(&toml_path)
        .arg(&payload_path)
        .arg(&out)
        .status()
        .expect("spawn nxb-pack");
    assert!(status.success(), "nxb-pack must succeed");
}

fn read_payload_kind(dir: &PathBuf) -> nexus_idl_runtime::manifest_capnp::PayloadKind {
    let bytes = std::fs::read(dir.join("out/manifest.nxb")).expect("manifest.nxb");
    let reader =
        capnp::serialize::read_message(bytes.as_slice(), Default::default()).expect("read");
    let manifest = reader
        .get_root::<nexus_idl_runtime::manifest_capnp::bundle_manifest::Reader<'_>>()
        .expect("root");
    manifest.get_payload_kind().expect("kind")
}

#[test]
fn ui_program_manifests_pack_payload_nxir_and_round_trip_the_kind() {
    let dir = temp_dir("uiprogram");
    run_pack(&dir, &manifest_toml(Some("ui-program")), b"fake-nxir-bytes");
    assert!(dir.join("out/payload.nxir").exists(), "DSL bundles ship payload.nxir");
    assert!(!dir.join("out/payload.elf").exists());
    assert_eq!(
        read_payload_kind(&dir),
        nexus_idl_runtime::manifest_capnp::PayloadKind::UiProgram
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn default_manifests_stay_elf_backward_compatible() {
    let dir = temp_dir("elfdefault");
    run_pack(&dir, &manifest_toml(None), b"\x7fELF fake");
    assert!(dir.join("out/payload.elf").exists(), "native bundles keep payload.elf");
    assert!(!dir.join("out/payload.nxir").exists());
    assert_eq!(
        read_payload_kind(&dir),
        nexus_idl_runtime::manifest_capnp::PayloadKind::Elf
    );
    let _ = std::fs::remove_dir_all(&dir);
}


fn run_pack_expect_failure(dir: &PathBuf, toml: &str) {
    let toml_path = dir.join("manifest.toml");
    let payload_path = dir.join("payload.bin");
    std::fs::write(&toml_path, toml).expect("write toml");
    std::fs::write(&payload_path, b"x").expect("write payload");
    let status = Command::new(env!("CARGO_BIN_EXE_nxb-pack"))
        .arg("--toml")
        .arg(&toml_path)
        .arg(&payload_path)
        .arg(dir.join("out"))
        .status()
        .expect("spawn nxb-pack");
    assert!(!status.success(), "nxb-pack must reject this manifest");
}

/// v2.2 (TASK-0081): the chat reference case — exports pack + survive the
/// digest rewrite; a foreign-namespace permission fails at PACK time.
#[test]
fn exports_round_trip_and_foreign_namespace_rejected() {
    let mut toml = manifest_toml(None);
    toml = toml.replace("name = \"demo.dslapp\"", "name = \"chat\"");
    toml.push_str(
        "exports = [\n    { ability = \"chat.Send\", permission = \"app.chat.SEND\" },\n    { ability = \"chat.Receive\", permission = \"app.chat.RECEIVE\" },\n]\n",
    );
    let dir = temp_dir("exports");
    run_pack(&dir, &toml, b"\x7fELF fake");
    let bytes = std::fs::read(dir.join("out/manifest.nxb")).expect("manifest.nxb");
    let reader =
        capnp::serialize::read_message(bytes.as_slice(), Default::default()).expect("read");
    let manifest = reader
        .get_root::<nexus_idl_runtime::manifest_capnp::bundle_manifest::Reader<'_>>()
        .expect("root");
    let exports = manifest.get_exports().expect("exports");
    assert_eq!(exports.len(), 2);
    assert_eq!(exports.get(0).get_ability().unwrap().to_str().unwrap(), "chat.Send");
    assert_eq!(exports.get(0).get_permission().unwrap().to_str().unwrap(), "app.chat.SEND");
    assert_eq!(exports.get(1).get_permission().unwrap().to_str().unwrap(), "app.chat.RECEIVE");
    let _ = std::fs::remove_dir_all(&dir);

    // Fail-closed at pack time: foreign namespace + empty CAP.
    let dir = temp_dir("exports-foreign");
    run_pack_expect_failure(&dir, &toml.replace("app.chat.SEND", "app.other.SEND"));
    let _ = std::fs::remove_dir_all(&dir);
    let dir = temp_dir("exports-empty");
    run_pack_expect_failure(&dir, &toml.replace("app.chat.SEND", "app.chat."));
    let _ = std::fs::remove_dir_all(&dir);
}
