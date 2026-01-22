// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Integration tests for system-set parsing and boot control flow
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 11 tests
//!
//! TEST_SCOPE:
//!   - System-set signature verification
//!   - Digest mismatch rejection
//!   - BootCtrl stage/switch/health
//!   - Rollback on health timeout
//!   - Missing signature rejection
//!   - Oversized archive rejection
//!   - Path-traversal rejection (security)
//!   - BootCtrl error states
//!
//! TEST_SCENARIOS:
//!   - test_stage_switch_health_commit(): happy-path flow
//!   - test_reject_invalid_signature(): bad signature rejected
//!   - test_reject_mismatched_digest(): digest mismatch rejected
//!   - test_rollback_on_health_timeout(): rollback after tries exhausted
//!   - test_reject_missing_signature(): signature entry missing
//!   - test_reject_oversized_archive(): oversized archive rejected
//!   - test_reject_path_traversal_dotdot(): ../ escape rejected
//!   - test_reject_absolute_path(): /etc/passwd rejected
//!   - test_bootctrl_switch_without_stage_fails(): error state
//!   - test_bootctrl_commit_health_without_switch_fails(): error state
//!   - test_bootctrl_double_switch_fails(): error state
//!
//! ADR: docs/rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md

use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use tar::{Builder as TarBuilder, EntryType, Header};

use updates::system_set_capnp::system_set_index;
use updates::SystemSetError;
use updates::{BootCtrl, Ed25519Verifier, Slot, SystemSet};

use capnp::message::Builder;
use capnp::serialize;
use nexus_idl_runtime::manifest_capnp::bundle_manifest;

#[derive(Clone)]
struct BundleFixture {
    name: String,
    version: String,
    manifest: Vec<u8>,
    payload: Vec<u8>,
}

#[test]
fn test_stage_switch_health_commit() {
    let signing_key = SigningKey::from_bytes(&[7u8; 32]);
    let bundle = fixture_bundle("demo.hello", "1.0.0");
    let nxs = build_nxs(&signing_key, &[bundle]);

    let verifier = Ed25519Verifier;
    let parsed = SystemSet::parse(&nxs, &verifier).expect("system-set parse ok");
    assert_eq!(parsed.bundles.len(), 1);

    let mut boot = BootCtrl::new(Slot::A);
    let staged = boot.stage();
    assert_eq!(staged, Slot::B);
    let switched = boot.switch(2).expect("switch ok");
    assert_eq!(switched, Slot::B);
    assert_eq!(boot.active_slot(), Slot::B);
    assert_eq!(boot.pending_slot(), Some(Slot::B));
    assert_eq!(boot.tries_left(), 2);
    boot.commit_health().expect("health ok");
    assert_eq!(boot.pending_slot(), None);
    assert!(boot.health_ok());
}

#[test]
fn test_reject_invalid_signature() {
    let signing_key = SigningKey::from_bytes(&[7u8; 32]);
    let bundle = fixture_bundle("demo.hello", "1.0.0");
    let mut bad_sig = [0u8; 64];
    bad_sig[0] = 0xFF;
    let nxs = build_nxs_with_signature(&signing_key, &[bundle], &bad_sig);

    let verifier = Ed25519Verifier;
    let err = SystemSet::parse(&nxs, &verifier).expect_err("signature rejection");
    assert!(matches!(err, updates::SystemSetError::InvalidSignature(_)));
}

#[test]
fn test_reject_mismatched_digest() {
    let signing_key = SigningKey::from_bytes(&[7u8; 32]);
    let bundle_index = fixture_bundle("demo.hello", "1.0.0");
    let mut bundle_archive = fixture_bundle("demo.hello", "1.0.0");
    bundle_archive.payload[0] ^= 0xFF;
    let nxs = build_nxs_with_index(&signing_key, &[bundle_index], &[bundle_archive]);

    let verifier = Ed25519Verifier;
    let err = SystemSet::parse(&nxs, &verifier).expect_err("digest mismatch");
    assert!(matches!(err, updates::SystemSetError::DigestMismatch { .. }));
}

#[test]
fn test_rollback_on_health_timeout() {
    let mut boot = BootCtrl::new(Slot::A);
    boot.stage();
    boot.switch(1).expect("switch ok");
    let rolled = boot.tick_boot_attempt().expect("tick").expect("rollback");
    assert_eq!(rolled, Slot::A);
    assert_eq!(boot.active_slot(), Slot::A);
    assert_eq!(boot.pending_slot(), None);
}

#[test]
fn test_reject_missing_signature() {
    let signing_key = SigningKey::from_bytes(&[7u8; 32]);
    let bundle = fixture_bundle("demo.hello", "1.0.0");
    let index_bytes = build_index(&signing_key.verifying_key().to_bytes(), &[bundle.clone()]);

    let mut tar = TarBuilder::new(Vec::new());
    append_file(&mut tar, "system.nxsindex", &index_bytes);
    append_dir(&mut tar, &format!("{}.nxb/", bundle.name));
    append_file(&mut tar, &format!("{}.nxb/manifest.nxb", bundle.name), &bundle.manifest);
    append_file(&mut tar, &format!("{}.nxb/payload.elf", bundle.name), &bundle.payload);
    let nxs = tar.into_inner().expect("tar bytes");

    let verifier = Ed25519Verifier;
    let err = SystemSet::parse(&nxs, &verifier).expect_err("missing signature");
    assert!(matches!(err, SystemSetError::MissingEntry("system.sig.ed25519")));
}

#[test]
fn test_reject_oversized_archive() {
    const MAX_NXS_ARCHIVE_BYTES: usize = 100 * 1024 * 1024;
    let nxs = vec![0u8; MAX_NXS_ARCHIVE_BYTES + 1];

    let verifier = Ed25519Verifier;
    let err = SystemSet::parse(&nxs, &verifier).expect_err("oversized archive");
    assert!(matches!(err, SystemSetError::ArchiveTooLarge { .. }));
}

#[test]
fn test_reject_path_traversal_dotdot() {
    let signing_key = SigningKey::from_bytes(&[7u8; 32]);
    let bundle = fixture_bundle("demo.hello", "1.0.0");
    let publisher = signing_key.verifying_key().to_bytes();
    let index_bytes = build_index(&publisher, &[bundle.clone()]);
    let signature = signing_key.sign(&index_bytes);

    let mut tar = TarBuilder::new(Vec::new());
    append_file(&mut tar, "system.nxsindex", &index_bytes);
    append_file(&mut tar, "system.sig.ed25519", &signature.to_bytes());
    // Malicious path with ../ - build raw tar entry
    append_file_raw(&mut tar, "../escape/payload.elf", &bundle.payload);
    let nxs = tar.into_inner().expect("tar bytes");

    let verifier = Ed25519Verifier;
    let err = SystemSet::parse(&nxs, &verifier).expect_err("path traversal");
    assert!(matches!(err, SystemSetError::ArchiveMalformed("unsafe path")));
}

#[test]
fn test_reject_absolute_path() {
    let signing_key = SigningKey::from_bytes(&[7u8; 32]);
    let bundle = fixture_bundle("demo.hello", "1.0.0");
    let publisher = signing_key.verifying_key().to_bytes();
    let index_bytes = build_index(&publisher, &[bundle.clone()]);
    let signature = signing_key.sign(&index_bytes);

    let mut tar = TarBuilder::new(Vec::new());
    append_file(&mut tar, "system.nxsindex", &index_bytes);
    append_file(&mut tar, "system.sig.ed25519", &signature.to_bytes());
    // Malicious absolute path - build raw tar entry
    append_file_raw(&mut tar, "/etc/passwd", &bundle.payload);
    let nxs = tar.into_inner().expect("tar bytes");

    let verifier = Ed25519Verifier;
    let err = SystemSet::parse(&nxs, &verifier).expect_err("absolute path");
    assert!(matches!(err, SystemSetError::ArchiveMalformed("unsafe path")));
}

#[test]
fn test_bootctrl_switch_without_stage_fails() {
    let mut boot = BootCtrl::new(Slot::A);
    let err = boot.switch(2).expect_err("should fail without stage");
    assert_eq!(err, updates::BootCtrlError::NotStaged);
}

#[test]
fn test_bootctrl_commit_health_without_switch_fails() {
    let mut boot = BootCtrl::new(Slot::A);
    let err = boot.commit_health().expect_err("should fail without switch");
    assert_eq!(err, updates::BootCtrlError::NotPending);
}

#[test]
fn test_bootctrl_double_switch_fails() {
    let mut boot = BootCtrl::new(Slot::A);
    boot.stage();
    boot.switch(2).expect("first switch ok");
    boot.stage();
    let err = boot.switch(2).expect_err("second switch should fail");
    assert_eq!(err, updates::BootCtrlError::AlreadyPending);
}

fn fixture_bundle(name: &str, version: &str) -> BundleFixture {
    let manifest = build_manifest(name, version);
    let payload = vec![0xAAu8; 16];
    BundleFixture { name: name.to_string(), version: version.to_string(), manifest, payload }
}

fn build_manifest(name: &str, version: &str) -> Vec<u8> {
    let mut builder = Builder::new_default();
    let mut msg = builder.init_root::<bundle_manifest::Builder>();
    msg.set_schema_version(1);
    msg.set_name(name);
    msg.set_semver(version);
    msg.set_min_sdk("1.0.0");
    msg.set_publisher(&[0u8; 32]);
    msg.set_signature(&[0u8; 64]);
    msg.reborrow().init_abilities(1).set(0, "demo");
    msg.reborrow().init_capabilities(0);

    let mut out = Vec::new();
    serialize::write_message(&mut out, &builder).expect("manifest encode");
    out
}

fn build_nxs(signing_key: &SigningKey, bundles: &[BundleFixture]) -> Vec<u8> {
    let signature =
        signing_key.sign(&build_index(&signing_key.verifying_key().to_bytes(), bundles));
    build_nxs_with_signature(signing_key, bundles, &signature.to_bytes())
}

fn build_nxs_with_signature(
    signing_key: &SigningKey,
    bundles: &[BundleFixture],
    signature_bytes: &[u8; 64],
) -> Vec<u8> {
    let publisher = signing_key.verifying_key().to_bytes();
    let index_bytes = build_index(&publisher, bundles);
    let mut tar = TarBuilder::new(Vec::new());
    append_file(&mut tar, "system.nxsindex", &index_bytes);
    append_file(&mut tar, "system.sig.ed25519", signature_bytes);

    for bundle in bundles {
        let dir_name = format!("{}.nxb/", bundle.name);
        append_dir(&mut tar, &dir_name);
        append_file(&mut tar, &format!("{}.nxb/manifest.nxb", bundle.name), &bundle.manifest);
        append_file(&mut tar, &format!("{}.nxb/payload.elf", bundle.name), &bundle.payload);
    }

    tar.into_inner().expect("tar bytes")
}

fn build_nxs_with_index(
    signing_key: &SigningKey,
    index_bundles: &[BundleFixture],
    archive_bundles: &[BundleFixture],
) -> Vec<u8> {
    let publisher = signing_key.verifying_key().to_bytes();
    let index_bytes = build_index(&publisher, index_bundles);
    let signature = signing_key.sign(&index_bytes);

    let mut tar = TarBuilder::new(Vec::new());
    append_file(&mut tar, "system.nxsindex", &index_bytes);
    append_file(&mut tar, "system.sig.ed25519", &signature.to_bytes());

    for bundle in archive_bundles {
        let dir_name = format!("{}.nxb/", bundle.name);
        append_dir(&mut tar, &dir_name);
        append_file(&mut tar, &format!("{}.nxb/manifest.nxb", bundle.name), &bundle.manifest);
        append_file(&mut tar, &format!("{}.nxb/payload.elf", bundle.name), &bundle.payload);
    }

    tar.into_inner().expect("tar bytes")
}

fn build_index(publisher: &[u8; 32], bundles: &[BundleFixture]) -> Vec<u8> {
    let mut builder = Builder::new_default();
    let mut root = builder.init_root::<system_set_index::Builder>();
    root.set_schema_version(1);
    root.set_system_version("1.0.0");
    root.set_publisher(publisher);
    root.set_timestamp_unix_ms(0);

    let mut list = root.reborrow().init_bundles(bundles.len() as u32);
    for (i, bundle) in bundles.iter().enumerate() {
        let mut entry = list.reborrow().get(i as u32);
        entry.set_name(&bundle.name);
        entry.set_version(&bundle.version);
        entry.set_manifest_sha256(&sha256(&bundle.manifest));
        entry.set_payload_sha256(&sha256(&bundle.payload));
        entry.set_payload_size(bundle.payload.len() as u64);
    }

    let mut out = Vec::new();
    serialize::write_message(&mut out, &builder).expect("index encode");
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
    builder.append_data(&mut header, path, std::io::empty()).expect("append dir");
}

/// Appends a file with a raw path, bypassing tar library path validation.
/// Used to test path-traversal rejection in the parser.
fn append_file_raw(builder: &mut TarBuilder<Vec<u8>>, path: &str, bytes: &[u8]) {
    use std::io::Write;

    // Build a raw tar header with the malicious path
    let mut header_bytes = [0u8; 512];
    // name (0..100)
    let path_bytes = path.as_bytes();
    let copy_len = std::cmp::min(path_bytes.len(), 100);
    header_bytes[..copy_len].copy_from_slice(&path_bytes[..copy_len]);
    // mode (100..108) = "0000644\0"
    header_bytes[100..108].copy_from_slice(b"0000644\0");
    // uid (108..116) = "0000000\0"
    header_bytes[108..116].copy_from_slice(b"0000000\0");
    // gid (116..124) = "0000000\0"
    header_bytes[116..124].copy_from_slice(b"0000000\0");
    // size (124..136) = octal size
    let size_str = format!("{:011o}\0", bytes.len());
    header_bytes[124..136].copy_from_slice(size_str.as_bytes());
    // mtime (136..148) = "00000000000\0"
    header_bytes[136..148].copy_from_slice(b"00000000000\0");
    // checksum placeholder (148..156) = spaces
    header_bytes[148..156].copy_from_slice(b"        ");
    // typeflag (156) = '0' (regular file)
    header_bytes[156] = b'0';
    // Calculate checksum
    let cksum: u32 = header_bytes.iter().map(|b| *b as u32).sum();
    let cksum_str = format!("{:06o}\0 ", cksum);
    header_bytes[148..156].copy_from_slice(cksum_str.as_bytes());

    // Write header + data + padding directly to the builder's inner buffer
    // We need to finish the current builder and rebuild with raw bytes
    let inner = builder.get_mut();
    inner.write_all(&header_bytes).expect("write header");
    inner.write_all(bytes).expect("write data");
    // Pad to 512-byte boundary
    let padding = (512 - (bytes.len() % 512)) % 512;
    inner.write_all(&vec![0u8; padding]).expect("write padding");
}
