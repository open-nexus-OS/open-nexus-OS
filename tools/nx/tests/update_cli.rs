// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Process-boundary tests for `nx update check/stage/switch/
//! status/rollback` (TASK-0140, RFC-0089 §8/§9): disk-truth status
//! decode, the provisioning-drop stage → feed check roundtrip through
//! the REAL device engine (untrusted publisher and downgrade-below-floor
//! rejects included), and the switch/rollback preflights that report
//! `applied=false` instead of pretending to drive a live device.
//! OWNERS: @tools-team @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 7 integration tests
//! ADR: docs/rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md

use std::path::Path;
use std::process::{Command, Output};

const OS_SEED_HEX: &str = "0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b";
/// The baked TRUSTED publisher (policies/update-trust.toml).
const PUB_SEED_HEX: &str = "0707070707070707070707070707070707070707070707070707070707070707";
/// Deliberately NOT in the device anchor (the deny lane's whole point).
const UNTRUSTED_SEED_HEX: &str = "0909090909090909090909090909090909090909090909090909090909090909";

fn run_nx(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nx"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("nx process must run")
}

fn stdout_json(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).expect("stdout must be the JSON envelope")
}

/// Built disk with floor 1 (so a rollback-index-0 container is a REAL
/// downgrade, mirroring the QEMU factory arrangement).
fn setup(dir: &Path) {
    std::fs::write(dir.join("os.seed"), OS_SEED_HEX).expect("os seed");
    std::fs::write(dir.join("pub.seed"), PUB_SEED_HEX).expect("pub seed");
    std::fs::write(dir.join("bad.seed"), UNTRUSTED_SEED_HEX).expect("bad seed");
    let kernel: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(dir.join("kernel.bin"), kernel).expect("kernel");
    let build = run_nx(
        &[
            "image",
            "build",
            "--kernel",
            "kernel.bin",
            "--out",
            "disk.img",
            "--sign",
            "os.seed",
            "--build-id",
            "dev-U",
            "--rollback-index",
            "1",
        ],
        dir,
    );
    assert!(build.status.success(), "image build: {build:?}");
}

fn make_container(dir: &Path, out: &str, pub_seed: &str, build_id: &str, rbidx: &str) {
    let ota = run_nx(
        &[
            "image",
            "ota",
            "--kernel",
            "kernel.bin",
            "--out",
            out,
            "--sign-publisher",
            pub_seed,
            "--sign-os",
            "os.seed",
            "--build-id",
            build_id,
            "--rollback-index",
            rbidx,
        ],
        dir,
    );
    assert!(ota.status.success(), "image ota: {ota:?}");
}

#[test]
fn status_reports_disk_truth() {
    let dir = tempfile::tempdir().expect("tempdir");
    setup(dir.path());
    let status = run_nx(
        &["update", "status", "--image", "disk.img", "--key", "os.seed", "--json"],
        dir.path(),
    );
    assert!(status.status.success(), "status: {status:?}");
    let v = stdout_json(&status);
    assert_eq!(v["data"]["bsb"]["active_slot"], "a");
    assert_eq!(v["data"]["bsb"]["rollback_min_index"], 1);
    assert_eq!(v["data"]["bsb"]["health_committed"], true);
    assert_eq!(v["data"]["slot_a"]["build_id"], "dev-U");
    assert_eq!(v["data"]["slot_a"]["sig"], "verified");
    assert_eq!(v["data"]["slot_b"]["empty"], true);
    // Without a key the descriptor decodes but stays honest about it.
    let unchecked = run_nx(&["update", "status", "--image", "disk.img", "--json"], dir.path());
    assert!(unchecked.status.success());
    assert_eq!(stdout_json(&unchecked)["data"]["slot_a"]["sig"], "unchecked");
}

#[test]
fn status_missing_image_is_missing_dependency() {
    let dir = tempfile::tempdir().expect("tempdir");
    let status = run_nx(&["update", "status", "--image", "nope.img", "--json"], dir.path());
    assert_eq!(status.status.code(), Some(4), "missing image: {status:?}");
    assert_eq!(stdout_json(&status)["class"], "missing_dependency");
}

#[test]
fn stage_then_check_roundtrip() {
    let dir = tempfile::tempdir().expect("tempdir");
    setup(dir.path());
    // Blank data partition = an honest empty feed.
    let empty = run_nx(&["update", "check", "--image", "disk.img", "--json"], dir.path());
    assert!(empty.status.success(), "check blank: {empty:?}");
    assert_eq!(stdout_json(&empty)["data"]["available"], 0);

    make_container(dir.path(), "up.nxs", "pub.seed", "otaU-2", "2");
    let stage = run_nx(&["update", "stage", "up.nxs", "--image", "disk.img", "--json"], dir.path());
    assert!(stage.status.success(), "stage: {stage:?}");
    let sv = stdout_json(&stage);
    assert_eq!(sv["data"]["path"], "/updates/up.nxs");
    assert_eq!(sv["data"]["build_id"], "otaU-2");

    let check = run_nx(&["update", "check", "--image", "disk.img", "--json"], dir.path());
    assert!(check.status.success(), "check: {check:?}");
    let cv = stdout_json(&check);
    assert_eq!(cv["data"]["available"], 1);
    assert_eq!(cv["data"]["candidates"][0]["name"], "up.nxs");
    assert_eq!(cv["data"]["candidates"][0]["verdict"], "ok");
    assert_eq!(cv["data"]["candidates"][0]["rollback_index"], 2);
}

#[test]
fn stage_rejects_untrusted_publisher() {
    let dir = tempfile::tempdir().expect("tempdir");
    setup(dir.path());
    make_container(dir.path(), "evil.nxs", "bad.seed", "evil-2", "2");
    let stage =
        run_nx(&["update", "stage", "evil.nxs", "--image", "disk.img", "--json"], dir.path());
    assert_eq!(stage.status.code(), Some(3), "untrusted stage: {stage:?}");
    let v = stdout_json(&stage);
    assert_eq!(v["class"], "validation_reject");
    assert!(
        v["message"].as_str().unwrap_or("").contains("untrusted publisher"),
        "stable reject vocabulary: {v}"
    );
    // The reject left no bytes behind: the feed stays empty.
    let check = run_nx(&["update", "check", "--image", "disk.img", "--json"], dir.path());
    assert_eq!(stdout_json(&check)["data"]["available"], 0);
}

#[test]
fn stage_rejects_downgrade_below_floor() {
    let dir = tempfile::tempdir().expect("tempdir");
    setup(dir.path());
    make_container(dir.path(), "old.nxs", "pub.seed", "old-0", "0");
    let stage =
        run_nx(&["update", "stage", "old.nxs", "--image", "disk.img", "--json"], dir.path());
    assert_eq!(stage.status.code(), Some(3), "downgrade stage: {stage:?}");
    assert!(
        stdout_json(&stage)["message"].as_str().unwrap_or("").contains("downgrade"),
        "stable reject vocabulary: {stage:?}"
    );
}

#[test]
fn switch_preflight_requires_valid_target_slot() {
    let dir = tempfile::tempdir().expect("tempdir");
    setup(dir.path());
    let empty = run_nx(&["update", "switch", "--image", "disk.img", "--json"], dir.path());
    assert_eq!(empty.status.code(), Some(3), "empty slot-b: {empty:?}");

    let patch = run_nx(
        &[
            "image",
            "patch",
            "--image",
            "disk.img",
            "--part",
            "boot-b",
            "--kernel",
            "kernel.bin",
            "--sign",
            "os.seed",
            "--build-id",
            "dev-U2",
            "--rollback-index",
            "2",
        ],
        dir.path(),
    );
    assert!(patch.status.success(), "patch: {patch:?}");
    let ok = run_nx(&["update", "switch", "--image", "disk.img", "--json"], dir.path());
    assert!(ok.status.success(), "switch preflight: {ok:?}");
    let v = stdout_json(&ok);
    assert_eq!(v["data"]["applied"], false, "a preflight NEVER claims it applied");
    assert_eq!(v["data"]["target"], "b");
    assert_eq!(v["data"]["build_id"], "dev-U2");
}

#[test]
fn rollback_preflight_requires_pending_trial() {
    let dir = tempfile::tempdir().expect("tempdir");
    setup(dir.path());
    let none = run_nx(&["update", "rollback", "--image", "disk.img", "--json"], dir.path());
    assert_eq!(none.status.code(), Some(3), "no trial: {none:?}");

    // `image backstop` arms a real trial (BSB next=b, tries=2).
    let arm = run_nx(
        &[
            "image",
            "backstop",
            "--image",
            "disk.img",
            "--kind",
            "tamper",
            "--kernel",
            "kernel.bin",
            "--sign",
            "os.seed",
            "--build-id",
            "dev-T2",
        ],
        dir.path(),
    );
    assert!(arm.status.success(), "backstop arm: {arm:?}");
    let ok = run_nx(&["update", "rollback", "--image", "disk.img", "--json"], dir.path());
    assert!(ok.status.success(), "rollback preflight: {ok:?}");
    let v = stdout_json(&ok);
    assert_eq!(v["data"]["applied"], false);
    assert_eq!(v["data"]["trial"], "b");
    assert_eq!(v["data"]["standing"], "a");
}
