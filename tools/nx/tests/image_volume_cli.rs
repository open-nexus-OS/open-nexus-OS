// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Process-boundary tests for the `nx image` system-volume half
//! (TASK-0321 P1, RFC-0089 §12): bundle directories → pkgimg v3 volume +
//! signed NXSV on system-a paired with boot-a (determinism, launch params
//! from `meta/launch.json` + the ELF's `.sdata`), `verify` walking every
//! digest and rejecting tampered / unpaired volumes, and `ota --bundle-set`
//! emitting `[boot-image, system-volume, bundle…]` whose kind-2 payloads
//! ARE the index bundle windows.
//! OWNERS: @reliability @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 3 integration tests (incl. patch re-pairing)
//! ADR: docs/adr/0060-verified-system-volume-bundlemgrd-verifier-init-spawner.md

use sha2::{Digest, Sha256};

#[path = "common/volume_fixture.rs"]
mod volume_fixture;
use volume_fixture::*;

#[test]
fn system_volume_build_is_deterministic_and_verifies() {
    let dir = tempfile::tempdir().expect("tempdir");
    setup(dir.path());
    setup_bundles(dir.path());
    let a = build_with_volume(dir.path(), "a.img", "dev-S");
    assert!(a.status.success(), "build a: {}", String::from_utf8_lossy(&a.stdout));
    assert!(build_with_volume(dir.path(), "b.img", "dev-S").status.success(), "build b");
    assert_eq!(file_sha(&dir.path().join("a.img")), file_sha(&dir.path().join("b.img")));
    assert!(dir.path().join("a.system-a.pkgimg").is_file(), "side file for the budget gate");

    let verify =
        run_nx(&["image", "verify", "--image", "a.img", "--key", "os.seed", "--json"], dir.path());
    assert!(verify.status.success(), "verify: {}", String::from_utf8_lossy(&verify.stdout));
    let v: serde_json::Value = serde_json::from_slice(&verify.stdout).expect("json");
    let sys = &v["data"]["system_a"];
    assert_eq!(sys["build_id"], "dev-S");
    assert_eq!(sys["paired"], true);
    let bundles = sys["bundles"].as_array().expect("bundles");
    assert_eq!(bundles.len(), 3);
    let metricsd = bundles.iter().find(|b| b["bundle"] == "metricsd").expect("metricsd row");
    assert_eq!(metricsd["stack_pages"], 8);
    assert_eq!(metricsd["global_pointer"], "0x80040800", "gp = .sdata + 0x800");
    let apps = bundles.iter().find(|b| b["bundle"] == "apps").expect("apps row");
    assert_eq!(apps["stack_pages"], 0, "data-only bundle is not spawnable");

    // A build WITHOUT bundles reports the slot as absent (factory-empty).
    assert!(build(dir.path(), "plain.img", "dev-P").status.success());
    let verify = run_nx(
        &["image", "verify", "--image", "plain.img", "--key", "os.seed", "--json"],
        dir.path(),
    );
    assert!(verify.status.success());
    let v: serde_json::Value = serde_json::from_slice(&verify.stdout).expect("json");
    assert_eq!(v["data"]["system_a"]["absent"], true);
}

#[test]
fn verify_rejects_tampered_and_unpaired_system_volume() {
    let dir = tempfile::tempdir().expect("tempdir");
    setup(dir.path());
    setup_bundles(dir.path());
    assert!(build_with_volume(dir.path(), "t.img", "dev-T").status.success());
    let img = dir.path().join("t.img");

    // Layout SSOT: bsb@1MiB, boot-a@2MiB(56MiB), boot-b@58MiB(56MiB),
    // system-a@114MiB; the volume body starts at sector 8. Patch ONE byte
    // in place (the image is 384 MiB — never slurp it in a test).
    let sys_a: u64 = 114 * 1024 * 1024;
    let off = sys_a + 8 * 512 + 8192 + 100; // inside a bundle window
    let flip = |xor: u8| {
        use std::io::{Read, Seek, SeekFrom, Write};
        let mut f = std::fs::OpenOptions::new().read(true).write(true).open(&img).expect("open");
        let mut b = [0u8; 1];
        f.seek(SeekFrom::Start(off)).unwrap();
        f.read_exact(&mut b).unwrap();
        f.seek(SeekFrom::Start(off)).unwrap();
        f.write_all(&[b[0] ^ xor]).unwrap();
    };
    flip(0xFF);
    let verify = run_nx(&["image", "verify", "--image", "t.img", "--key", "os.seed"], dir.path());
    assert!(!verify.status.success(), "tampered volume must fail verify");
    let msg = String::from_utf8_lossy(&verify.stdout);
    assert!(msg.contains("digest"), "{msg}");
    flip(0xFF); // restore

    // Unpaired: refresh boot-a with a different image → NXSV pairing breaks.
    let other: Vec<u8> = (0..300_000u32).map(|i| (i % 241) as u8).collect();
    std::fs::write(dir.path().join("other.bin"), other).expect("other kernel");
    let patch = run_nx(
        &[
            "image",
            "patch",
            "--image",
            "t.img",
            "--part",
            "boot-a",
            "--kernel",
            "other.bin",
            "--sign",
            "os.seed",
            "--build-id",
            "dev-T2",
        ],
        dir.path(),
    );
    assert!(patch.status.success(), "patch: {patch:?}");
    let verify = run_nx(&["image", "verify", "--image", "t.img", "--key", "os.seed"], dir.path());
    assert!(!verify.status.success(), "unpaired volume must fail verify");
    assert!(String::from_utf8_lossy(&verify.stdout).contains("pair"));

    // The keep-blk / flasher shape: `patch --system-bundles` refreshes the
    // paired system slot together with the boot slot → verify is green again.
    let patch = run_nx(
        &[
            "image",
            "patch",
            "--image",
            "t.img",
            "--part",
            "boot-a",
            "--kernel",
            "other.bin",
            "--sign",
            "os.seed",
            "--build-id",
            "dev-T2",
            "--system-bundles",
            "bundles",
        ],
        dir.path(),
    );
    assert!(patch.status.success(), "patch+volume: {patch:?}");
    let verify =
        run_nx(&["image", "verify", "--image", "t.img", "--key", "os.seed", "--json"], dir.path());
    assert!(verify.status.success(), "re-paired: {}", String::from_utf8_lossy(&verify.stdout));
    let v: serde_json::Value = serde_json::from_slice(&verify.stdout).expect("json");
    assert_eq!(v["data"]["system_a"]["build_id"], "dev-T2");
    assert_eq!(v["data"]["system_a"]["paired"], true);
}

#[test]
fn bundle_set_container_decodes_kinds_2_and_6() {
    let dir = tempfile::tempdir().expect("tempdir");
    setup(dir.path());
    setup_bundles(dir.path());
    let ota = run_nx(
        &[
            "image",
            "ota",
            "--kernel",
            "kernel.bin",
            "--out",
            "set.nxs",
            "--sign-publisher",
            "pub.seed",
            "--sign-os",
            "os.seed",
            "--build-id",
            "dev-S",
            "--rollback-index",
            "2",
            "--bundle-set",
            "bundles",
        ],
        dir.path(),
    );
    assert!(ota.status.success(), "ota: {}", String::from_utf8_lossy(&ota.stdout));
    let bytes = std::fs::read(dir.path().join("set.nxs")).expect("read nxs");
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
    assert_eq!(
        names,
        vec![
            "manifest.nxo",
            "manifest.sig.ed25519",
            "boot.img",
            "system.idx",
            "bundles/apps@1.0.0.bin",
            "bundles/metricsd@1.0.0.bin",
            "bundles/timed@1.0.0.bin",
        ],
        "§12.4 ordering: boot image, system-volume, bundles (sorted)"
    );
    let reader = capnp::serialize::read_message(
        &mut contents[0].as_slice(),
        capnp::message::ReaderOptions::new(),
    )
    .expect("capnp read");
    let manifest = reader
        .get_root::<updates::system_set_capnp::component_manifest::Reader>()
        .expect("manifest root");
    let components = manifest.get_components().expect("components");
    assert_eq!(components.len(), 5);
    let kinds: Vec<u8> = (0..components.len()).map(|i| components.get(i).get_kind()).collect();
    assert_eq!(kinds, vec![1, 6, 2, 2, 2]);

    // Kind 6: payload = superblock + index; kindData = NXSV verifying under
    // the OS key, index digest == component digest, paired with boot.img.
    let sv = components.get(1);
    let os_key = ed25519_dalek::SigningKey::from_bytes(&[0x0bu8; 32]).verifying_key().to_bytes();
    let nxsv = bootfmt::nxsv::verify(sv.get_kind_data().expect("nxsv"), &os_key)
        .expect("embedded nxsv verifies");
    let mut h = Sha256::new();
    h.update(&contents[3]);
    let idx_digest: [u8; 32] = h.finalize().into();
    assert_eq!(nxsv.index_sha256, idx_digest);
    assert_eq!(nxsv.index_len as usize, contents[3].len());
    assert_eq!(sv.get_sha256().expect("sha"), &idx_digest[..]);
    assert_eq!(nxsv.rollback_index, 2);
    let mut h = Sha256::new();
    h.update(&contents[2]);
    let boot_digest: [u8; 32] = h.finalize().into();
    assert_eq!(nxsv.boot_image_sha256, boot_digest, "system volume pairs with the boot image");

    // Kind 2: each payload is exactly a bundle window — its digest equals
    // the index bundle-table digest.
    let index =
        storage::pkgimg_bundles::parse_index(&contents[3], &storage::pkgimg::PkgImgCaps::default())
            .expect("index parses standalone");
    for (i, name) in ["apps", "metricsd", "timed"].iter().enumerate() {
        let c = components.get(2 + i as u32);
        assert_eq!(c.get_name().expect("name").to_str().unwrap(), format!("{name}@1.0.0"));
        let row = index.bundle(name, "1.0.0").expect("bundle row");
        let mut h = Sha256::new();
        h.update(&contents[4 + i]);
        let d: [u8; 32] = h.finalize().into();
        assert_eq!(d, row.sha256, "component payload == bundle window");
        assert_eq!(c.get_size(), row.data_len);
    }
}
