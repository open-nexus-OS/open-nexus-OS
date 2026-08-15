// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: fsck-statefs CLI contract (TASK-0026): stable exit codes
//! (0 clean / 1 repaired-or-orphan / 2 unrecoverable), deterministic report
//! lines, `--dry-run` never writes, `--repair` writes back an image that
//! re-fscks clean. Fixtures are byte-exact images built via the statefs
//! engine API; raw byte flips only where the corruption is the fixture.
//! OWNERS: @runtime
//! STATUS: Functional
//! TEST_COVERAGE: this file IS the coverage (deterministic, no randomness)

use std::path::PathBuf;
use std::process::Command;

use statefs::{JournalEngine, FSCK_BLOCK_SIZE};
use storage::{BlockDevice, MemBlockDevice};

const BLOCKS: u64 = 64;

/// Serialize a device into flat image bytes.
fn dump(device: &MemBlockDevice) -> Vec<u8> {
    let mut out = Vec::with_capacity(FSCK_BLOCK_SIZE * device.block_count() as usize);
    let mut block = vec![0u8; FSCK_BLOCK_SIZE];
    for idx in 0..device.block_count() {
        device.read_block(idx, &mut block).expect("read");
        out.extend_from_slice(&block);
    }
    out
}

/// Write fixture bytes to a per-test file under the system temp dir.
fn fixture(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fsck-statefs-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("write fixture");
    path
}

/// Run the fsck-statefs binary; returns (exit code, stdout).
fn run(args: &[&str]) -> (i32, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_fsck-statefs"))
        .args(args)
        .output()
        .expect("run fsck-statefs");
    let code = output.status.code().expect("exit code");
    (code, String::from_utf8(output.stdout).expect("utf8 stdout"))
}

/// Legacy v1 journal with two committed keys.
fn v1_image() -> MemBlockDevice {
    let mut engine =
        JournalEngine::open(MemBlockDevice::new(FSCK_BLOCK_SIZE, BLOCKS)).expect("open");
    engine.put("/state/test/keep", b"stable content").expect("put");
    engine.put("/state/boot/slot", b"A").expect("put");
    engine.sync().expect("sync");
    engine.into_device()
}

/// v1 journal ending in an orphaned transaction; returns (bytes, txn id).
fn orphan_image_bytes() -> (Vec<u8>, u64) {
    let mut engine = JournalEngine::open(v1_image()).expect("open");
    let txn = engine.txn_begin().expect("begin");
    engine.txn_append(txn, "/state/test/torn", b"never committed").expect("append");
    (dump(&engine.into_device()), txn)
}

#[test]
fn clean_v1_image_exits_0() {
    let path = fixture("clean-v1.img", &dump(&v1_image()));
    let (code, stdout) = run(&[path.to_str().expect("path")]);
    assert_eq!(code, 0, "stdout: {stdout}");
    assert!(stdout.contains("layout=v1 generation=0 records=2 entries=2"), "stdout: {stdout}");
    assert!(stdout.contains("outcome=Clean"), "stdout: {stdout}");
}

#[test]
fn clean_v2_image_exits_0() {
    let mut engine = JournalEngine::open(v1_image()).expect("open");
    engine.compact_now().expect("compact");
    let path = fixture("clean-v2.img", &dump(&engine.into_device()));
    let (code, stdout) = run(&[path.to_str().expect("path")]);
    assert_eq!(code, 0, "stdout: {stdout}");
    assert!(stdout.contains("layout=v2 generation=1 records=3 entries=2"), "stdout: {stdout}");
    assert!(stdout.contains("outcome=Clean"), "stdout: {stdout}");
}

#[test]
fn orphan_reported_exit_1_without_writing() {
    let (bytes, txn) = orphan_image_bytes();
    let path = fixture("orphan-report.img", &bytes);
    let (code, stdout) = run(&[path.to_str().expect("path")]);
    assert_eq!(code, 1, "stdout: {stdout}");
    let orphan_line = format!("orphan txn {txn} (PREPARE/PAYLOAD without COMMIT/ABORT)");
    let plan_line = format!("would repair: append TXN_ABORT for txn {txn}");
    assert!(stdout.contains(&orphan_line), "stdout: {stdout}");
    assert!(stdout.contains(&plan_line), "stdout: {stdout}");
    assert_eq!(std::fs::read(&path).expect("read"), bytes, "no-repair run must not write");
}

#[test]
fn dry_run_with_repair_flag_never_writes() {
    let (bytes, txn) = orphan_image_bytes();
    let path = fixture("orphan-dry-run.img", &bytes);
    let (code, stdout) = run(&["--repair", "--dry-run", path.to_str().expect("path")]);
    assert_eq!(code, 1, "stdout: {stdout}");
    let plan_line = format!("would repair: append TXN_ABORT for txn {txn}");
    assert!(stdout.contains(&plan_line), "stdout: {stdout}");
    assert!(!stdout.contains("repaired: appended"), "stdout: {stdout}");
    assert_eq!(std::fs::read(&path).expect("read"), bytes, "--dry-run must not write");
}

#[test]
fn repair_exits_1_and_image_re_fscks_clean() {
    let (bytes, txn) = orphan_image_bytes();
    let path = fixture("orphan-repair.img", &bytes);
    let (code, stdout) = run(&["--repair", path.to_str().expect("path")]);
    assert_eq!(code, 1, "stdout: {stdout}");
    let repaired_line = format!("repaired: appended TXN_ABORT for txn {txn}");
    assert!(stdout.contains(&repaired_line), "stdout: {stdout}");
    assert_ne!(std::fs::read(&path).expect("read"), bytes, "repair writes the image back");

    // The repaired image replays clean: exit 0, orphan gone.
    let (code, stdout) = run(&[path.to_str().expect("path")]);
    assert_eq!(code, 0, "stdout: {stdout}");
    assert!(stdout.contains("outcome=Clean"), "stdout: {stdout}");
    assert!(stdout.contains("orphans=0 anomalies=0"), "stdout: {stdout}");
}

#[test]
fn test_reject_corrupted_crc_exits_2() {
    let mut bytes = dump(&v1_image());
    // Flip a key byte inside the FIRST record (header is 15 bytes).
    bytes[17] ^= 0xFF;
    let path = fixture("corrupt-crc.img", &bytes);
    let (code, stdout) = run(&[path.to_str().expect("path")]);
    assert_eq!(code, 2, "stdout: {stdout}");
    assert!(stdout.contains("unrecoverable at byte offset 0: crc mismatch"), "stdout: {stdout}");
}

#[test]
fn test_reject_damaged_superblock_exits_2() {
    let mut engine = JournalEngine::open(v1_image()).expect("open");
    engine.compact_now().expect("compact");
    let mut bytes = dump(&engine.into_device());
    // Damage the generation field; the superblock CRC no longer matches.
    bytes[8] ^= 0x01;
    let path = fixture("bad-superblock.img", &bytes);
    let (code, stdout) = run(&[path.to_str().expect("path")]);
    assert_eq!(code, 2, "stdout: {stdout}");
    assert!(
        stdout.contains("unrecoverable at byte offset 0: invalid superblock"),
        "stdout: {stdout}"
    );
}

#[test]
fn test_reject_unaligned_image_length_exits_2() {
    let mut bytes = dump(&v1_image());
    bytes.truncate(bytes.len() - 100); // no longer a multiple of 512
    let path = fixture("unaligned.img", &bytes);
    let (code, _) = run(&[path.to_str().expect("path")]);
    assert_eq!(code, 2);
}

#[test]
fn test_reject_missing_file_and_bad_usage_exit_2() {
    let (code, _) = run(&["/nonexistent/fsck-statefs-test.img"]);
    assert_eq!(code, 2);
    let (code, _) = run(&["--bogus-flag", "whatever.img"]);
    assert_eq!(code, 2);
    let (code, _) = run(&[]);
    assert_eq!(code, 2);
}
