// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Process-boundary tests for `nx diagnose` (TASK-0051): a
//! fixture statefs image (boot record + evidence ring, built through the
//! SAME SSOT encoders the OS uses) must yield a deterministic ustar bundle
//! with the expected sections; missing/invalid images map to the stable
//! nx exit classes. The fsck OUTCOME is data inside the bundle, never the
//! exit code.
//! OWNERS: @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 4 integration tests
//! ADR: docs/adr/0055-bootctld-single-boot-state-authority.md

use std::path::Path;
use std::process::{Command, Output};

use logd::journal::{Journal, LogLevel, TimestampNsec};
use logd::spill::SpillEngine;
use storage::{BlockDevice, MemBlockDevice};

fn run_nx(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nx"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("nx process must run")
}

/// Fixture image: one boot record + two evidence records, written through
/// the delivered engine/encoder stack (no hand-rolled bytes).
fn fixture_image(dir: &Path) -> std::path::PathBuf {
    let mut engine =
        statefs::JournalEngine::open(MemBlockDevice::new(512, 256)).expect("open store");

    let boot = bootctld::machine::BootCtrl::new(bootctld::machine::Slot::A);
    let sealed = bootctld::record::seal_record(&boot, 1, 42).expect("seal boot record");
    engine.put(bootctld::record::BOOT_RECORD_KEY, &sealed).expect("put boot record");

    let mut journal = Journal::new(8, 8192);
    let mut spill = SpillEngine::new(0);
    for (msg, fields) in [
        (&b"crash pid=60"[..], &b"event=crash.v1\ncode=-22\n"[..]),
        (&b"degrade gl->2d"[..], &b"event=exhaust.v1\nreason=arena\n"[..]),
    ] {
        journal
            .append(0x77, TimestampNsec(42), LogLevel::Warn, b"execd", msg, fields)
            .expect("append");
        let record = journal.iter_since(TimestampNsec(0)).last().expect("record").clone();
        let plan = spill.plan(&record);
        engine.put(plan.slot_key.as_str(), &plan.value).expect("put slot");
        engine.put(logd::spill::HEAD_KEY, &plan.head_value).expect("put head");
    }
    engine.sync().expect("sync");

    let device = engine.into_device();
    let block_count = device.block_count();
    let mut bytes = Vec::new();
    let mut block = vec![0u8; 512];
    for i in 0..block_count {
        device.read_block(i, &mut block).expect("read block");
        bytes.extend_from_slice(&block);
    }
    let path = dir.join("blk.img");
    std::fs::write(&path, &bytes).expect("write image");
    path
}

/// Entry names in archive order (ustar headers at 512-byte boundaries).
fn tar_entry_names(archive: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    let mut off = 0usize;
    while off + 512 <= archive.len() {
        let header = &archive[off..off + 512];
        if header.iter().all(|&b| b == 0) {
            break;
        }
        let name_end = header.iter().position(|&b| b == 0).unwrap_or(100).min(100);
        names.push(String::from_utf8_lossy(&header[..name_end]).into_owned());
        let size_field = std::str::from_utf8(&header[124..135]).expect("octal size");
        let size = usize::from_str_radix(size_field, 8).expect("size parses");
        off += 512 + size.div_ceil(512) * 512;
    }
    names
}

#[test]
fn diagnose_bundle_is_deterministic_with_expected_sections() {
    let dir = tempfile::tempdir().expect("tempdir");
    let image = fixture_image(dir.path());
    let image_arg = image.display().to_string();

    let first =
        run_nx(&["diagnose", "--image", &image_arg, "--out", "a.tar", "--json"], dir.path());
    assert!(first.status.success(), "stderr: {}", String::from_utf8_lossy(&first.stderr));
    let second =
        run_nx(&["diagnose", "--image", &image_arg, "--out", "b.tar", "--json"], dir.path());
    assert!(second.status.success());

    let a = std::fs::read(dir.path().join("a.tar")).expect("read a");
    let b = std::fs::read(dir.path().join("b.tar")).expect("read b");
    assert_eq!(a, b, "same inputs must yield a byte-identical bundle");

    assert_eq!(
        tar_entry_names(&a),
        vec![
            "diagnose/boot-record.json".to_string(),
            "diagnose/evidence.jsonl".to_string(),
            "diagnose/fsck-report.json".to_string(),
            "diagnose/meta.json".to_string(),
        ]
    );

    let text = String::from_utf8_lossy(&a);
    assert!(text.contains("\"active_slot\": \"a\""), "boot record section decoded");
    assert!(text.contains("event=crash.v1"), "evidence records present");
    assert!(text.contains("\"outcome\": \"clean\""), "fsck verdict is data");

    let envelope: serde_json::Value = serde_json::from_slice(&first.stdout).expect("json envelope");
    assert_eq!(envelope["data"]["fsck_outcome"], "clean");
    assert_eq!(envelope["data"]["evidence_records"], 2);
    assert_eq!(envelope["data"]["boot_record_present"], true);
}

#[test]
fn test_reject_missing_image_maps_missing_dependency() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run_nx(&["diagnose", "--image", "no-such.img", "--out", "x.tar"], dir.path());
    assert_eq!(out.status.code(), Some(4), "missing image = missing_dependency");
}

#[test]
fn test_reject_unaligned_image_maps_validation_reject() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("bad.img");
    std::fs::write(&path, [0u8; 700]).expect("write");
    let out =
        run_nx(&["diagnose", "--image", &path.display().to_string(), "--out", "x.tar"], dir.path());
    assert_eq!(out.status.code(), Some(3), "unaligned image = validation_reject");
}

#[test]
fn diagnose_unreplayable_store_still_bundles_fsck_verdict() {
    let dir = tempfile::tempdir().expect("tempdir");
    // All-zero image: no journal to replay — sections go absent, the
    // command still succeeds and the fsck verdict lands in the bundle.
    let path = dir.path().join("zero.img");
    std::fs::write(&path, vec![0u8; 512 * 64]).expect("write");
    let out = run_nx(
        &["diagnose", "--image", &path.display().to_string(), "--out", "z.tar", "--json"],
        dir.path(),
    );
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let envelope: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(envelope["data"]["evidence_records"], 0);
    let archive = std::fs::read(dir.path().join("z.tar")).expect("read");
    assert_eq!(tar_entry_names(&archive).len(), 4, "all sections present even when absent");
}
