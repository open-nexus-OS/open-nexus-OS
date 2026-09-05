// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Process-boundary tests for the bundle-set REUSE half (TASK-0321
//! P3/P4a fixtures, TASK-0035 P2 host reuse index): `nx image fixtures
//! --system-bundles --reuse-from` (self-verified `bundle-set.nxs` + os-B
//! carrying the factory volume) and `nx image ota --bundle-set --reuse-from`
//! (only changed windows ship; the reuse manifest names the rest — against a
//! built disk image and against a bundle directory).
//! OWNERS: @reliability @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 2 integration tests
//! ADR: docs/adr/0060-verified-system-volume-bundlemgrd-verifier-init-spawner.md

use std::path::Path;

#[path = "common/volume_fixture.rs"]
mod volume_fixture;
use volume_fixture::*;

/// TASK-0321 P3: `image fixtures --system-bundles` emits `bundle-set.nxs`
/// (os-B + system-volume + bundles) and SELF-VERIFIES it through the real
/// device engine against the baked publisher anchor — so the pairing of
/// the NXSV with os-B's digest and the §12.4 ordering are proven at
/// factory time, not first in QEMU.
#[test]
fn fixtures_emit_a_self_verified_bundle_set() {
    let dir = tempfile::tempdir().expect("tempdir");
    setup(dir.path());
    let root = dir.path().join("bundles");
    make_bundle(&root, "metricsd", "1.0.1", true, &tiny_elf(0x8004_0000));
    // The device anchor is the repo's dev publisher key (policies/update-
    // trust.toml) — fixtures must verify against exactly that.
    let publisher = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../keys/dev-publisher.ed25519.seed")
        .canonicalize()
        .expect("dev publisher seed");
    let out = run_nx(
        &[
            "image",
            "fixtures",
            "--kernel",
            "kernel.bin",
            "--data-out",
            "data-seed.img",
            "--data-mib",
            "32",
            "--sign-os",
            "os.seed",
            "--sign-publisher",
            publisher.to_str().expect("utf8 path"),
            "--build-id",
            "dev-abcdef",
            "--system-bundles",
            "bundles",
            "--json",
        ],
        dir.path(),
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "fixtures: {stdout}");
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
    let names: Vec<&str> = json["data"]["containers"]
        .as_array()
        .expect("containers")
        .iter()
        .map(|c| c["name"].as_str().expect("name"))
        .collect();
    assert!(names.contains(&"os-B.nxs"), "{names:?}");
    assert!(names.contains(&"bundle-set.nxs"), "{names:?}");
    assert_eq!(json["data"]["build_b"].as_str().expect("build_b"), "otaBabcdef");
    // TASK-0321 P4: with the factory set as `--reuse-from`, only the bundle
    // whose window changed ships; the unchanged one is a device reuse.
    let base = dir.path().join("base");
    make_bundle(&base, "metricsd", "1.0.0", true, &tiny_elf(0x8004_0000));
    make_bundle(&base, "timed", "1.0.0", true, &tiny_elf(0x8008_0000));
    make_bundle(&root, "timed", "1.0.0", true, &tiny_elf(0x8008_0000));
    let out = run_nx(
        &[
            "image",
            "fixtures",
            "--kernel",
            "kernel.bin",
            "--data-out",
            "data-seed3.img",
            "--data-mib",
            "32",
            "--sign-os",
            "os.seed",
            "--sign-publisher",
            publisher.to_str().expect("utf8 path"),
            "--build-id",
            "dev-abcdef",
            "--system-bundles",
            "bundles",
            "--reuse-from",
            "base",
            "--json",
        ],
        dir.path(),
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "fixtures: {stdout}");
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
    assert_eq!(json["data"]["bundle_set"]["shipped"], serde_json::json!(["metricsd@1.0.1"]));
    assert_eq!(json["data"]["bundle_set"]["reused"], serde_json::json!(["timed@1.0.0"]));
    // os-B carries the FACTORY volume paired with itself: 2 bundles, both reused.
    assert_eq!(json["data"]["os_b"]["volume_bundles"], 2);
    assert_eq!(json["data"]["os_b"]["reused"], 2);
    // Without the bundle directory no bundle set is emitted (the lane
    // that stages it must fail loudly, never silently stage os-B).
    let out = run_nx(
        &[
            "image",
            "fixtures",
            "--kernel",
            "kernel.bin",
            "--data-out",
            "data-seed2.img",
            "--data-mib",
            "32",
            "--sign-os",
            "os.seed",
            "--sign-publisher",
            publisher.to_str().expect("utf8 path"),
            "--build-id",
            "dev-abcdef",
            "--json",
        ],
        dir.path(),
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "fixtures: {stdout}");
    assert!(!stdout.contains("bundle-set.nxs"));
}

/// TASK-0035 P2: `ota --bundle-set --reuse-from <active disk image>` ships
/// ONLY the bundles whose window digest changed against the device's
/// `system-a`; the reuse manifest names the rest — and the same diff works
/// against a bundle directory.
#[test]
fn bundle_set_ships_only_changed_bundles() {
    let dir = tempfile::tempdir().expect("tempdir");
    setup(dir.path());
    setup_bundles(dir.path());
    // The device's volume: a built disk with system-a from the v1 set.
    let out = build_with_volume(dir.path(), "nexus.img", "dev-A");
    assert!(out.status.success(), "build: {}", String::from_utf8_lossy(&out.stdout));
    // The NEXT set: metricsd bumped, timed/apps unchanged.
    let next = dir.path().join("next");
    make_bundle(&next, "metricsd", "1.0.1", true, &tiny_elf(0x8004_0000));
    make_bundle(&next, "timed", "1.0.0", true, &tiny_elf(0x8008_0000));
    make_bundle(&next, "apps", "1.0.0", false, b"ui-program-bytes");
    for base in ["nexus.img", "bundles"] {
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
                "next",
                "--reuse-from",
                base,
                "--json",
            ],
            dir.path(),
        );
        let stdout = String::from_utf8_lossy(&ota.stdout);
        assert!(ota.status.success(), "ota ({base}): {stdout}");
        let json: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
        assert_eq!(
            json["data"]["bundle_set"]["shipped"],
            serde_json::json!(["metricsd@1.0.1"]),
            "{base}"
        );
        assert_eq!(
            json["data"]["bundle_set"]["reused"],
            serde_json::json!(["apps@1.0.0", "timed@1.0.0"]),
            "{base}"
        );
        // The container carries exactly boot.img + system.idx + the one bundle.
        let bytes = std::fs::read(dir.path().join("set.nxs")).expect("read nxs");
        let mut archive = tar::Archive::new(&bytes[..]);
        let names: Vec<String> = archive
            .entries()
            .expect("entries")
            .map(|e| e.expect("entry").path().expect("path").display().to_string())
            .collect();
        assert_eq!(
            names,
            vec![
                "manifest.nxo",
                "manifest.sig.ed25519",
                "boot.img",
                "system.idx",
                "bundles/metricsd@1.0.1.bin",
            ],
            "{base}"
        );
    }
    // A disk without a system volume is rejected loudly as a reuse base.
    let blank = dir.path().join("blank.img");
    std::fs::write(&blank, vec![0u8; 4096]).unwrap();
    let ota = run_nx(
        &[
            "image",
            "ota",
            "--kernel",
            "kernel.bin",
            "--out",
            "x.nxs",
            "--sign-publisher",
            "pub.seed",
            "--sign-os",
            "os.seed",
            "--build-id",
            "dev-S",
            "--rollback-index",
            "2",
            "--bundle-set",
            "next",
            "--reuse-from",
            "blank.img",
        ],
        dir.path(),
    );
    assert!(!ota.status.success());
}
