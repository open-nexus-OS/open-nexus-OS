// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Build-time generator for a deterministic system-test `.nxs` archive.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests
//!
//! This keeps the on-device selftest asset in sync with schema and payload changes.

// Build scripts may use expect/unwrap since failures are hard errors.
#![allow(clippy::expect_used)]

use std::{env, fs, path::PathBuf};

use capnp::message::Builder;
use capnp::serialize;
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use tar::{Builder as TarBuilder, EntryType, Header};

use exec_payloads::HELLO_MANIFEST_NXB;
use updates::system_set_capnp::system_set_index;

// SECURITY: bring-up test key for deterministic selftests (NOT production custody).
const TEST_SIGNING_KEY_SEED: [u8; 32] = [7u8; 32];

// Keep the selftest `.nxs` small so OTA staging stays fast under QEMU emulation.
// This payload is NOT executed; it is only hashed and packaged.
const TEST_PAYLOAD: &[u8] = b"open-nexus-os selftest payload v1\n";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../../userspace/exec-payloads/src/hello_elf.rs");
    println!("cargo:rerun-if-changed=../../../userspace/exec-payloads/build.rs");
    println!("cargo:rerun-if-changed=../../../tools/nexus-idl/schemas/system-set.capnp");
    println!("cargo:rerun-if-changed=../../../tools/nexus-idl/schemas/manifest.capnp");

    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let nxs_bytes = build_system_test_nxs();
    fs::write(out_dir.join("system-test.nxs"), nxs_bytes)?;
    Ok(())
}

fn build_system_test_nxs() -> Vec<u8> {
    let signing_key = SigningKey::from_bytes(&TEST_SIGNING_KEY_SEED);
    let publisher = signing_key.verifying_key().to_bytes();

    let index_bytes = build_index(&publisher, HELLO_MANIFEST_NXB, TEST_PAYLOAD);
    let signature = signing_key.sign(&index_bytes);
    let signature_bytes = signature.to_bytes();

    let mut tar = TarBuilder::new(Vec::new());
    append_file(&mut tar, "system.nxsindex", &index_bytes);
    append_file(&mut tar, "system.sig.ed25519", &signature_bytes);

    let dir_name = "demo.hello.nxb/";
    append_dir(&mut tar, dir_name);
    append_file(&mut tar, "demo.hello.nxb/manifest.nxb", HELLO_MANIFEST_NXB);
    append_file(&mut tar, "demo.hello.nxb/payload.elf", TEST_PAYLOAD);

    tar.into_inner().expect("system-test.nxs bytes")
}

fn build_index(publisher: &[u8; 32], manifest: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut builder = Builder::new_default();
    let mut root = builder.init_root::<system_set_index::Builder>();
    root.set_schema_version(1);
    root.set_system_version("1.0.0");
    root.set_publisher(publisher);
    root.set_timestamp_unix_ms(0);

    let mut bundles = root.reborrow().init_bundles(1);
    let mut entry = bundles.reborrow().get(0);
    entry.set_name("demo.hello");
    entry.set_version("0.0.1");
    entry.set_manifest_sha256(&sha256(manifest));
    entry.set_payload_sha256(&sha256(payload));
    entry.set_payload_size(payload.len() as u64);

    let mut out = Vec::new();
    serialize::write_message(&mut out, &builder).expect("system.nxsindex encode");
    out
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

fn append_file(builder: &mut TarBuilder<Vec<u8>>, path: &str, bytes: &[u8]) {
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Regular);
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_cksum();
    builder.append_data(&mut header, path, bytes).expect("append file");
}

fn append_dir(builder: &mut TarBuilder<Vec<u8>>, path: &str) {
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Directory);
    header.set_size(0);
    header.set_mode(0o755);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_cksum();
    builder.append_data(&mut header, path, &[] as &[u8]).expect("append dir");
}
