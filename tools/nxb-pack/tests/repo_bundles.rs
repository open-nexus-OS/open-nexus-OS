// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// CONTEXT: Proves the repo app bundles (`userspace/apps/<app>/manifest.toml`) pack into
// a real `.nxb` with a valid Cap'n Proto `manifest.nxb` carrying the app id +
// launch ability — the data `bundlemgrd` enumerates and `abilitymgr` resolves
// from (RFC-0065 — chat/search as real apps).

use std::path::PathBuf;
use std::process::Command;

use capnp::message::ReaderOptions;
use capnp::serialize;
use nexus_idl_runtime::manifest_capnp::bundle_manifest;

/// Repo root (two levels up from this crate: `tools/nxb-pack`).
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("repo root")
}

/// Packs `userspace/apps/<app>/manifest.toml` with a placeholder payload and returns the
/// parsed manifest fields `(name, abilities, capabilities)`.
fn pack_and_read(app: &str) -> (String, Vec<String>, Vec<String>) {
    let root = repo_root();
    let toml = root.join("userspace/apps").join(app).join("manifest.toml");
    assert!(toml.is_file(), "missing bundle manifest: {}", toml.display());

    let tmp = tempfile::tempdir().expect("tempdir");
    let payload = tmp.path().join("placeholder.elf");
    std::fs::write(&payload, b"\x7fELF placeholder payload").expect("write payload");
    let out = tmp.path().join(format!("{app}.nxb"));

    let status = Command::new(env!("CARGO_BIN_EXE_nxb-pack"))
        .arg("--toml")
        .arg(&toml)
        .arg(&payload)
        .arg(&out)
        .status()
        .expect("run nxb-pack");
    assert!(status.success(), "nxb-pack failed for {app}");

    let manifest_path = out.join("manifest.nxb");
    assert!(manifest_path.is_file(), "no manifest.nxb produced for {app}");
    let bytes = std::fs::read(&manifest_path).expect("read manifest.nxb");

    let mut cursor = std::io::Cursor::new(bytes);
    let message = serialize::read_message(&mut cursor, ReaderOptions::new()).expect("capnp read");
    let m = message.get_root::<bundle_manifest::Reader<'_>>().expect("manifest root");

    let name = m.get_name().expect("name").to_str().expect("name utf8").to_string();
    let abilities_reader = m.get_abilities().expect("abilities");
    let abilities = (0..abilities_reader.len())
        .map(|i| abilities_reader.get(i).unwrap().to_str().unwrap().to_string())
        .collect();
    let caps_reader = m.get_capabilities().expect("caps");
    let caps = (0..caps_reader.len())
        .map(|i| caps_reader.get(i).unwrap().to_str().unwrap().to_string())
        .collect();
    (name, abilities, caps)
}

#[test]
fn chat_bundle_packs_with_launch_ability() {
    let (name, abilities, caps) = pack_and_read("chat");
    assert_eq!(name, "chat");
    assert_eq!(abilities, vec!["chat.MainAbility".to_string()]);
    assert_eq!(caps, vec!["nexus.permission.WINDOW".to_string()]);
}

#[test]
fn stash_bundle_packs_as_filemanager_with_files_cap() {
    // The filemanager role (RFC-0073/TASK-0291): stash packs with FILES
    // because its bundle_type ceiling allows it.
    let (name, abilities, caps) = pack_and_read("stash");
    assert_eq!(name, "stash");
    assert_eq!(abilities, vec!["stash.MainAbility".to_string()]);
    assert!(caps.contains(&"nexus.permission.FILES".to_string()), "caps: {caps:?}");
}

#[test]
fn test_reject_files_cap_for_plain_app_bundle_type() {
    // Privilege ceiling: a plain `app` may not ship FILES — pack fails closed.
    let tmp = tempfile::tempdir().expect("tempdir");
    let toml = tmp.path().join("manifest.toml");
    std::fs::write(
        &toml,
        r#"name = "rogue"
version = "0.1.0"
min_sdk = "1.0.0"
bundle_type = "app"
abilities = ["rogue.MainAbility"]
caps = ["nexus.permission.FILES"]
"#,
    )
    .expect("write manifest");
    let payload = tmp.path().join("placeholder.elf");
    std::fs::write(&payload, b"\x7fELF placeholder payload").expect("write payload");
    let out = tmp.path().join("rogue.nxb");

    let status = Command::new(env!("CARGO_BIN_EXE_nxb-pack"))
        .arg("--toml")
        .arg(&toml)
        .arg(&payload)
        .arg(&out)
        .status()
        .expect("run nxb-pack");
    assert!(!status.success(), "FILES on bundle_type=app must fail the pack");
}
