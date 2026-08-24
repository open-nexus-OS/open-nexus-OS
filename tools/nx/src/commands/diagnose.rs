// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `nx diagnose` — THE one diagnostic bundle (TASK-0051, Keystone
//! Gate 6: nx is the only diagnostics CLI). Host-side assembly over the
//! delivered SSOT decoders — statefs journal replay (engine crate), logd
//! evidence ring (`logd::spill::decode_slot_value`), bootctld boot record
//! (`bootctld::record::open_record`), fsck verdict (`statefs::fsck`) — no
//! on-device bundler, no second bundle format (TASK-0227 keeps the format
//! ownership and rebases onto these sections). Deterministic by
//! construction: same image bytes ⇒ byte-identical archive (hand-rolled
//! ustar, mtime 0, fixed order, no host timestamps). The fsck OUTCOME is
//! data (fsck-report.json), not the exit class — nx exit classes stay the
//! CLI contract, fsck exit codes stay with fsck-statefs.
//! OWNERS: @reliability @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tests/diagnose_cli.rs (deterministic archive, sections,
//! rejects).
//! ADR: docs/adr/0055-bootctld-single-boot-state-authority.md

use serde_json::{json, Value};
use storage::{BlockDevice, MemBlockDevice};

use crate::cli::DiagnoseArgs;
use crate::error::{ExecResult, ExitClass, NxError};

/// Bundle layout version (meta.json `bundle_version`).
const BUNDLE_VERSION: u32 = 1;

pub(crate) fn handle_diagnose(args: DiagnoseArgs) -> ExecResult {
    let bytes = std::fs::read(&args.image).map_err(|err| {
        NxError::new(
            ExitClass::MissingDependency,
            format!("diagnose: read {}: {err}", args.image.display()),
        )
    })?;
    if bytes.is_empty() || bytes.len() % statefs::FSCK_BLOCK_SIZE != 0 {
        return Err(NxError::new(
            ExitClass::ValidationReject,
            format!(
                "diagnose: {} is not a {}-byte-block statefs image",
                args.image.display(),
                statefs::FSCK_BLOCK_SIZE
            ),
        ));
    }

    // fsck consumes its device; the store readers get a second one — both
    // from the same bytes, so the sections cannot diverge.
    let (fsck_report, _) = statefs::fsck(load_device(&bytes), false);
    let store = read_store_sections(load_device(&bytes));

    let mut entries: Vec<(String, Vec<u8>)> = vec![
        ("diagnose/boot-record.json".to_string(), pretty(&store.boot_record)),
        ("diagnose/evidence.jsonl".to_string(), store.evidence_jsonl),
        ("diagnose/fsck-report.json".to_string(), pretty(&fsck_json(&fsck_report))),
        (
            "diagnose/meta.json".to_string(),
            pretty(&json!({
                "bundle_version": BUNDLE_VERSION,
                "tool": "nx diagnose",
                "image_blocks": bytes.len() / statefs::FSCK_BLOCK_SIZE,
                "sections": ["boot-record", "evidence", "fsck-report", "crash"],
            })),
        ),
    ];
    // TASK-0051B: at-rest crash artifacts ride the bundle verbatim — after
    // `tar -x`, `nx crash ls/show` symbolizes them host-side.
    for (name, artifact) in &store.crash_artifacts {
        entries.push((format!("crash/{name}"), artifact.clone()));
    }
    let archive = ustar_archive(&entries);
    std::fs::write(&args.out, &archive).map_err(|err| {
        NxError::new(ExitClass::Internal, format!("diagnose: write {}: {err}", args.out.display()))
    })?;

    let data = json!({
        "out": args.out.display().to_string(),
        "bytes": archive.len(),
        "fsck_outcome": outcome_label(fsck_report.outcome),
        "evidence_records": store.evidence_count,
        "boot_record_present": store.boot_record_present,
        "crash_artifacts": store.crash_artifacts.len(),
    });
    Ok((
        ExitClass::Success,
        format!("diagnose: bundle written to {}", args.out.display()),
        args.json,
        Some(data),
    ))
}

fn load_device(bytes: &[u8]) -> MemBlockDevice {
    let block_count = (bytes.len() / statefs::FSCK_BLOCK_SIZE) as u64;
    let mut device = MemBlockDevice::new(statefs::FSCK_BLOCK_SIZE, block_count);
    for (idx, chunk) in bytes.chunks(statefs::FSCK_BLOCK_SIZE).enumerate() {
        // Cannot fail: the device was sized from the same byte length.
        let _ = device.write_block(idx as u64, chunk);
    }
    device
}

struct StoreSections {
    boot_record: Value,
    boot_record_present: bool,
    evidence_jsonl: Vec<u8>,
    evidence_count: usize,
    /// At-rest crash artifacts (`.nxcd`/degraded `.nmd`), basename → bytes,
    /// key-sorted for deterministic bundle order.
    crash_artifacts: Vec<(String, Vec<u8>)>,
}

/// Reads the boot record + evidence ring through the SSOT decoders. A store
/// that does not replay yields honest `absent` sections — the fsck report
/// carries the verdict.
fn read_store_sections(device: MemBlockDevice) -> StoreSections {
    let absent = StoreSections {
        boot_record: json!({ "present": false }),
        boot_record_present: false,
        evidence_jsonl: Vec::new(),
        evidence_count: 0,
        crash_artifacts: Vec::new(),
    };
    let Ok(engine) = statefs::JournalEngine::open(device) else {
        return absent;
    };

    let (boot_record, boot_record_present) = match engine.get(bootctld::record::BOOT_RECORD_KEY) {
        Ok(bytes) => match bootctld::record::open_record(&bytes) {
            Ok((boot, seq)) => (boot_json(&boot, seq), true),
            Err(_) => (json!({ "present": true, "decodable": false }), true),
        },
        Err(_) => (json!({ "present": false }), false),
    };

    // Evidence ring: head names the live slot count; records are emitted in
    // spill-sequence order (ascending, oldest first).
    let mut lines = Vec::new();
    let mut count = 0usize;
    if let Ok(head_bytes) = engine.get(logd::spill::HEAD_KEY) {
        let head = u64_le(&head_bytes);
        let live = logd::spill::SpillEngine::live_slots(head);
        for seq in head.saturating_sub(live)..head {
            let key =
                format!("{}{:02}", logd::spill::SLOT_KEY_PREFIX, seq % logd::spill::MAX_SLOTS);
            let Ok(value) = engine.get(&key) else { continue };
            let Some(record) = logd::spill::decode_slot_value(&value) else { continue };
            if record.seq != seq {
                // Slot overwritten by a newer wrap while head lagged — the
                // ring is drop-oldest, a mismatched seq is stale.
                continue;
            }
            lines.extend_from_slice(evidence_line(&record).as_bytes());
            lines.push(b'\n');
            count += 1;
        }
    }

    // Crash artifacts at rest (TASK-0051B): bounded LIST, key order is the
    // bundle order (statefs list is prefix-sorted; sort defensively).
    let mut crash_artifacts = Vec::new();
    if let Ok(mut keys) = engine.list("/state/crash/", 64) {
        keys.sort();
        for key in keys {
            let Ok(bytes) = engine.get(&key) else { continue };
            let name = key.rsplit('/').next().unwrap_or(&key).to_string();
            crash_artifacts.push((name, bytes));
        }
    }

    StoreSections {
        boot_record,
        boot_record_present,
        evidence_jsonl: lines,
        evidence_count: count,
        crash_artifacts,
    }
}

fn evidence_line(record: &logd::spill::SpilledRecord) -> String {
    json!({
        "seq": record.seq,
        "level": level_label(record.level),
        "service_id": record.service_id,
        "timestamp_nsec": record.timestamp_nsec,
        "scope": String::from_utf8_lossy(&record.scope),
        "message": String::from_utf8_lossy(&record.message),
        "fields": String::from_utf8_lossy(&record.fields),
    })
    .to_string()
}

fn level_label(level: logd::journal::LogLevel) -> &'static str {
    match level {
        logd::journal::LogLevel::Error => "error",
        logd::journal::LogLevel::Warn => "warn",
        logd::journal::LogLevel::Info => "info",
        logd::journal::LogLevel::Debug => "debug",
        logd::journal::LogLevel::Trace => "trace",
    }
}

fn boot_json(boot: &bootctld::machine::BootCtrl, seq: Option<u64>) -> Value {
    json!({
        "present": true,
        "decodable": true,
        "active_slot": slot_label(boot.active_slot()),
        "pending_slot": boot.pending_slot().map(slot_label),
        "staged_slot": boot.staged_slot().map(slot_label),
        "rollback_slot": boot.rollback_slot().map(slot_label),
        "tries_left": boot.tries_left(),
        "health_ok": boot.health_ok(),
        "boot_target": target_label(boot.boot_target()),
        "next_boot": boot.next_boot().map(target_label),
        "envelope_seq": seq,
    })
}

fn slot_label(slot: bootctld::machine::Slot) -> &'static str {
    match slot {
        bootctld::machine::Slot::A => "a",
        bootctld::machine::Slot::B => "b",
    }
}

fn target_label(target: bootctld::machine::BootTarget) -> &'static str {
    match target {
        bootctld::machine::BootTarget::Normal => "normal",
        bootctld::machine::BootTarget::Recovery => "recovery",
        bootctld::machine::BootTarget::Safe => "safe",
    }
}

fn fsck_json(report: &statefs::FsckReport) -> Value {
    json!({
        "outcome": outcome_label(report.outcome),
        "layout": match report.layout {
            statefs::JournalLayout::V1 => 1,
            statefs::JournalLayout::V2 => 2,
        },
        "generation": report.generation,
        "records": report.records,
        "entries": report.entries,
        "orphan_txns": report.orphan_txns,
        "anomalies": report.anomalies,
        "tail_dirty": report.tail_dirty,
        "enc_records": report.enc_records,
        "enc_failures": report.enc_failures,
        "fault": report.fault.map(|f| json!({ "offset": f.offset, "reason": f.reason })),
    })
}

fn outcome_label(outcome: statefs::FsckOutcome) -> &'static str {
    match outcome {
        statefs::FsckOutcome::Clean => "clean",
        statefs::FsckOutcome::Repaired => "repaired",
        statefs::FsckOutcome::Unrecoverable => "unrecoverable",
    }
}

fn pretty(value: &Value) -> Vec<u8> {
    let mut out = serde_json::to_vec_pretty(value).unwrap_or_else(|_| b"{}".to_vec());
    out.push(b'\n');
    out
}

fn u64_le(bytes: &[u8]) -> u64 {
    let mut buf = [0u8; 8];
    let len = bytes.len().min(8);
    buf[..len].copy_from_slice(&bytes[..len]);
    u64::from_le_bytes(buf)
}

// ============================================================================
// Deterministic ustar writer (POSIX.1-1988 layout, fixed metadata)
// ============================================================================

/// Serializes `entries` (already in fixed bundle order) as a ustar archive:
/// mode 0644, uid/gid 0, mtime 0 — byte-identical for identical inputs.
fn ustar_archive(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, data) in entries {
        out.extend_from_slice(&ustar_header(name, data.len() as u64));
        out.extend_from_slice(data);
        let pad = (512 - data.len() % 512) % 512;
        out.resize(out.len() + pad, 0);
    }
    // End-of-archive: two zero blocks.
    out.resize(out.len() + 1024, 0);
    out
}

fn ustar_header(name: &str, size: u64) -> [u8; 512] {
    let mut h = [0u8; 512];
    let name_bytes = name.as_bytes();
    let n = name_bytes.len().min(100);
    h[..n].copy_from_slice(&name_bytes[..n]);
    octal(&mut h[100..108], 0o644); // mode
    octal(&mut h[108..116], 0); // uid
    octal(&mut h[116..124], 0); // gid
    octal12(&mut h[124..136], size);
    octal12(&mut h[136..148], 0); // mtime
    h[148..156].fill(b' '); // checksum placeholder
    h[156] = b'0'; // typeflag: regular file
    h[257..262].copy_from_slice(b"ustar");
    h[263..265].copy_from_slice(b"00");
    let sum: u32 = h.iter().map(|&b| b as u32).sum();
    let mut chk = [0u8; 8];
    octal(&mut chk, sum);
    chk[7] = b' ';
    h[148..156].copy_from_slice(&chk);
    h
}

/// 8-byte octal field: 6 digits + NUL + implicit trailing byte.
fn octal(field: &mut [u8], value: u32) {
    let mut v = value;
    field[6] = 0;
    for i in (0..6).rev() {
        field[i] = b'0' + (v % 8) as u8;
        v /= 8;
    }
}

/// 12-byte octal field: 11 digits + NUL.
fn octal12(field: &mut [u8], value: u64) {
    let mut v = value;
    field[11] = 0;
    for i in (0..11).rev() {
        field[i] = b'0' + (v % 8) as u8;
        v /= 8;
    }
}
