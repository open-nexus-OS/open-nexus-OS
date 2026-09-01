// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Process-boundary tests for `nx image build/verify/patch/ota`
//! (TASK-0260, RFC-0089): determinism (build twice ⇒ identical bytes),
//! verify round-trip through the SHARED gpt parser, tamper/downgrade-shape
//! rejects, patch preserving bsb/state/data byte-identically, slot-budget
//! enforcement, and `.nxs` v2 container decode via the updates crate.
//! OWNERS: @reliability @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 6 integration tests
//! ADR: docs/adr/0058-boot-selection-block-dual-actor-discipline.md

use std::path::Path;
use std::process::{Command, Output};

use sha2::{Digest, Sha256};

const OS_SEED_HEX: &str = "0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b";
const PUB_SEED_HEX: &str = "0707070707070707070707070707070707070707070707070707070707070707";

fn run_nx(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nx"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("nx process must run")
}

fn setup(dir: &Path) {
    std::fs::write(dir.join("os.seed"), OS_SEED_HEX).expect("os seed");
    std::fs::write(dir.join("pub.seed"), PUB_SEED_HEX).expect("pub seed");
    // Deterministic fake boot image (not executed; hashed + packaged).
    let kernel: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(dir.join("kernel.bin"), kernel).expect("kernel");
}

fn file_sha(path: &Path) -> [u8; 32] {
    let bytes = std::fs::read(path).expect("read");
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    hasher.finalize().into()
}

fn build(dir: &Path, out: &str, build_id: &str) -> Output {
    run_nx(
        &[
            "image",
            "build",
            "--kernel",
            "kernel.bin",
            "--out",
            out,
            "--sign",
            "os.seed",
            "--build-id",
            build_id,
        ],
        dir,
    )
}

#[test]
fn build_is_deterministic_and_verifies() {
    let dir = tempfile::tempdir().expect("tempdir");
    setup(dir.path());
    assert!(build(dir.path(), "a.img", "dev-A").status.success(), "build a");
    assert!(build(dir.path(), "b.img", "dev-A").status.success(), "build b");
    assert_eq!(
        file_sha(&dir.path().join("a.img")),
        file_sha(&dir.path().join("b.img")),
        "build twice must be byte-identical"
    );
    let verify =
        run_nx(&["image", "verify", "--image", "a.img", "--key", "os.seed", "--json"], dir.path());
    assert!(verify.status.success(), "verify: {verify:?}");
    let v: serde_json::Value = serde_json::from_slice(&verify.stdout).expect("json");
    assert_eq!(v["data"]["boot_a"]["build_id"], "dev-A");
    assert_eq!(v["data"]["bsb"]["active_slot"], "a");
    assert_eq!(v["data"]["bsb"]["seq"], 1);
}

#[test]
fn verify_rejects_tampered_boot_image() {
    let dir = tempfile::tempdir().expect("tempdir");
    setup(dir.path());
    assert!(build(dir.path(), "t.img", "dev-T").status.success());
    // Flip one byte INSIDE the boot-a payload (bsb sits at 1 MiB, boot-a
    // starts at the next 1 MiB alignment = 2 MiB; image at sector 8).
    let img = dir.path().join("t.img");
    let mut bytes = std::fs::read(&img).expect("read img");
    let off = 2 * 1024 * 1024 + 8 * 512 + 1000;
    bytes[off] ^= 0xFF;
    std::fs::write(&img, bytes).expect("write img");
    let verify = run_nx(&["image", "verify", "--image", "t.img", "--key", "os.seed"], dir.path());
    assert!(!verify.status.success(), "tampered image must fail verify");
}

#[test]
fn verify_rejects_wrong_key() {
    let dir = tempfile::tempdir().expect("tempdir");
    setup(dir.path());
    assert!(build(dir.path(), "k.img", "dev-K").status.success());
    std::fs::write(dir.path().join("other.seed"), "0c".repeat(32)).expect("seed");
    let verify =
        run_nx(&["image", "verify", "--image", "k.img", "--key", "other.seed"], dir.path());
    assert!(!verify.status.success(), "wrong key must fail nxbd verify");
}

#[test]
fn patch_refreshes_boot_a_and_preserves_everything_else() {
    let dir = tempfile::tempdir().expect("tempdir");
    setup(dir.path());
    assert!(build(dir.path(), "p.img", "dev-P1").status.success());
    let before = std::fs::read(dir.path().join("p.img")).expect("read");

    let kernel2: Vec<u8> = (0..200_000u32).map(|i| (i % 241) as u8).collect();
    std::fs::write(dir.path().join("kernel2.bin"), kernel2).expect("kernel2");
    let patch = run_nx(
        &[
            "image",
            "patch",
            "--image",
            "p.img",
            "--part",
            "boot-a",
            "--kernel",
            "kernel2.bin",
            "--sign",
            "os.seed",
            "--build-id",
            "dev-P2",
        ],
        dir.path(),
    );
    assert!(patch.status.success(), "patch: {patch:?}");
    let after = std::fs::read(dir.path().join("p.img")).expect("read");
    assert_eq!(before.len(), after.len());

    // bsb partition (1 MiB region before boot-a) byte-identical…
    let bsb_start = 2048 * 512;
    let boot_a_start = 2 * 1024 * 1024; // second aligned partition start
    assert_eq!(
        before[bsb_start..boot_a_start],
        after[bsb_start..boot_a_start],
        "bsb region untouched"
    );
    // …and everything after boot-a (boot-b/system/state/data) too.
    let boot_a_end = boot_a_start + 56 * 1024 * 1024;
    assert_eq!(before[boot_a_end..], after[boot_a_end..], "later partitions untouched");
    // The refreshed build id is visible.
    let verify =
        run_nx(&["image", "verify", "--image", "p.img", "--key", "os.seed", "--json"], dir.path());
    assert!(verify.status.success());
    let v: serde_json::Value = serde_json::from_slice(&verify.stdout).expect("json");
    assert_eq!(v["data"]["boot_a"]["build_id"], "dev-P2");
}

#[test]
fn build_rejects_kernel_over_slot_budget() {
    let dir = tempfile::tempdir().expect("tempdir");
    setup(dir.path());
    // 57 MiB > 56 MiB slot budget.
    let big = vec![0x5Au8; 57 * 1024 * 1024];
    std::fs::write(dir.path().join("kernel.bin"), big).expect("big kernel");
    let out = build(dir.path(), "big.img", "dev-BIG");
    assert!(!out.status.success(), "over-budget kernel must fail the build");
}

#[test]
fn ota_container_decodes_with_bound_nxbd() {
    let dir = tempfile::tempdir().expect("tempdir");
    setup(dir.path());
    let ota = run_nx(
        &[
            "image",
            "ota",
            "--kernel",
            "kernel.bin",
            "--out",
            "os-B.nxs",
            "--sign-publisher",
            "pub.seed",
            "--sign-os",
            "os.seed",
            "--build-id",
            "dev-B",
            "--rollback-index",
            "3",
        ],
        dir.path(),
    );
    assert!(ota.status.success(), "ota: {ota:?}");

    // Decode the container: entry order, publisher signature, manifest
    // fields, NXBD binding (kindData verifies under the OS key and matches
    // the component digest/size).
    let bytes = std::fs::read(dir.path().join("os-B.nxs")).expect("read nxs");
    let mut archive = tar::Archive::new(&bytes[..]);
    let mut names = Vec::new();
    let mut contents: Vec<Vec<u8>> = Vec::new();
    for entry in archive.entries().expect("entries") {
        let mut entry = entry.expect("entry");
        names.push(entry.path().expect("path").display().to_string());
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut buf).expect("read entry");
        contents.push(buf);
    }
    assert_eq!(names, vec!["manifest.nxo", "manifest.sig.ed25519", "boot.img"]);

    use ed25519_dalek::{Signature, Signer, SigningKey, Verifier};
    let publisher = SigningKey::from_bytes(&[0x07u8; 32]);
    let sig_bytes: [u8; 64] = contents[1].as_slice().try_into().expect("64-byte sig");
    publisher
        .verifying_key()
        .verify(&contents[0], &Signature::from_bytes(&sig_bytes))
        .expect("publisher signature binds manifest.nxo");
    // Determinism note: the same seed signs identically (no RNG).
    assert_eq!(publisher.sign(&contents[0]).to_bytes(), sig_bytes);

    let reader = capnp::serialize::read_message(
        &mut contents[0].as_slice(),
        capnp::message::ReaderOptions::new(),
    )
    .expect("capnp read");
    let manifest = reader
        .get_root::<updates::system_set_capnp::component_manifest::Reader>()
        .expect("manifest root");
    assert_eq!(manifest.get_schema_version(), 2);
    assert_eq!(manifest.get_rollback_index(), 3);
    assert_eq!(manifest.get_build_id().expect("build id").to_str().expect("utf8"), "dev-B");
    let components = manifest.get_components().expect("components");
    assert_eq!(components.len(), 1);
    let c = components.get(0);
    assert_eq!(c.get_kind(), 1);
    assert_eq!(c.get_size() as usize, contents[2].len());
    let mut hasher = Sha256::new();
    hasher.update(&contents[2]);
    let digest: [u8; 32] = hasher.finalize().into();
    assert_eq!(c.get_sha256().expect("sha"), &digest[..]);

    let os_key = SigningKey::from_bytes(&[0x0bu8; 32]).verifying_key().to_bytes();
    let nxbd_bytes = c.get_kind_data().expect("kind data");
    let desc = bootfmt::nxbd::verify(nxbd_bytes, &os_key).expect("embedded nxbd verifies");
    assert_eq!(desc.rollback_index, 3);
    assert_eq!(desc.image_size as usize, contents[2].len());
    assert_eq!(desc.image_sha256, digest);
    assert_eq!(desc.build_id_str(), "dev-B");
}

#[test]
fn backstop_arms_the_alternate_bsb_block_and_keeps_the_floor() {
    let dir = tempfile::tempdir().expect("tempdir");
    setup(dir.path());
    // Factory floor 1 (the shipped image's index) — the downgrade backstop
    // plants rollback index 0 below it.
    assert!(run_nx(
        &[
            "image",
            "build",
            "--kernel",
            "kernel.bin",
            "--out",
            "bs.img",
            "--sign",
            "os.seed",
            "--build-id",
            "dev-A",
            "--rollback-index",
            "1",
        ],
        dir.path(),
    )
    .status
    .success());
    for (kind, want_idx) in [("tamper", 2u64), ("downgrade", 0u64)] {
        let out = run_nx(
            &[
                "image",
                "backstop",
                "--image",
                "bs.img",
                "--kind",
                kind,
                "--kernel",
                "kernel.bin",
                "--sign",
                "os.seed",
                "--build-id",
                "trialB01",
                "--json",
            ],
            dir.path(),
        );
        assert!(out.status.success(), "backstop {kind}: {out:?}");
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json");
        assert_eq!(v["data"]["nxbd_rollback_index"], want_idx, "{kind}");
        // The floor travels from the factory block into the armed block —
        // losing it would disarm the very check the downgrade lane proves.
        assert_eq!(v["data"]["bsb_floor"], 1, "{kind}");
    }
    // Two arms = two alternate-block writes; the reader must see the
    // newest (seq 3) with the trial pending and the floor intact.
    let bytes = std::fs::read(dir.path().join("bs.img")).expect("img");
    let bsb_off = 1024 * 1024; // first partition (layout SSOT: bsb at 1 MiB)
    let (bsb, _) =
        bootfmt::bsb::pick(&bytes[bsb_off..bsb_off + 512], &bytes[bsb_off + 512..bsb_off + 1024])
            .expect("armed bsb must decode");
    assert_eq!(bsb.seq, 3);
    assert_eq!(bsb.next_slot, Some(bootfmt::bsb::Slot::B));
    assert_eq!(bsb.tries_left, 2);
    assert_eq!(bsb.rollback_min_index, 1);
}
