// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: fsck-statefs CLI — offline validation/repair of statefs journal
//! images in host workflows (TASK-0026; byte contract docs/storage/
//! statefs.md §Journal v2). Thin shell over `statefs::fsck` (the core lives
//! in the engine crate, mirroring fsck-nxfs). Deterministic report, stable
//! exit codes: 0 clean / 1 repaired-or-orphan / 2 unrecoverable. Repair is
//! append-only (`TXN_ABORT` per orphan) and never rewrites committed data;
//! `--dry-run` reports what a repair would append without writing.
//! OWNERS: @runtime
//! STATUS: Functional
//! TEST_COVERAGE: tests/cli.rs (exit-code + output contract) +
//! userspace/statefs/tests/fsck.rs (core outcome matrix)

use std::process::ExitCode;

use statefs::{fsck, FsckOutcome, JournalLayout, FSCK_BLOCK_SIZE};
use storage::{BlockDevice, MemBlockDevice};

fn usage() -> ExitCode {
    eprintln!("usage: fsck-statefs [--repair] [--dry-run] <journal-image>");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let mut repair = false;
    let mut dry_run = false;
    let mut image_path: Option<String> = None;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--repair" => repair = true,
            "--dry-run" => dry_run = true,
            other if image_path.is_none() && !other.starts_with('-') => {
                image_path = Some(other.to_string());
            }
            _ => return usage(),
        }
    }
    let Some(path) = image_path else {
        return usage();
    };

    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("fsck-statefs: read {path}: {err}");
            return ExitCode::from(2);
        }
    };
    if bytes.is_empty() || bytes.len() % FSCK_BLOCK_SIZE != 0 {
        eprintln!(
            "fsck-statefs: {path}: image length {} is not a multiple of the {FSCK_BLOCK_SIZE}-byte block",
            bytes.len()
        );
        return ExitCode::from(2);
    }

    let block_count = (bytes.len() / FSCK_BLOCK_SIZE) as u64;
    let mut device = MemBlockDevice::new(FSCK_BLOCK_SIZE, block_count);
    for (idx, chunk) in bytes.chunks(FSCK_BLOCK_SIZE).enumerate() {
        if device.write_block(idx as u64, chunk).is_err() {
            eprintln!("fsck-statefs: {path}: image load failed");
            return ExitCode::from(2);
        }
    }

    // --dry-run reports the repair plan without touching the image.
    let apply_repair = repair && !dry_run;
    let (report, device) = fsck(device, apply_repair);

    let layout = match report.layout {
        JournalLayout::V1 => "v1",
        JournalLayout::V2 => "v2",
    };
    println!(
        "fsck-statefs: {path}: layout={layout} generation={} records={} entries={} orphans={} anomalies={} tail_dirty={} outcome={:?}",
        report.generation,
        report.records,
        report.entries,
        report.orphan_txns.len(),
        report.anomalies,
        report.tail_dirty,
        report.outcome
    );
    for id in &report.orphan_txns {
        println!("fsck-statefs: {path}: orphan txn {id} (PREPARE/PAYLOAD without COMMIT/ABORT)");
        if report.repaired {
            println!("fsck-statefs: {path}: repaired: appended TXN_ABORT for txn {id}");
        } else {
            println!("fsck-statefs: {path}: would repair: append TXN_ABORT for txn {id}");
        }
    }
    if let Some(fault) = report.fault {
        println!(
            "fsck-statefs: {path}: unrecoverable at byte offset {}: {}",
            fault.offset, fault.reason
        );
    }

    if report.repaired {
        let Some(device) = device else {
            eprintln!("fsck-statefs: {path}: repaired image unavailable");
            return ExitCode::from(2);
        };
        let mut out = Vec::with_capacity(bytes.len());
        let mut block = vec![0u8; FSCK_BLOCK_SIZE];
        for idx in 0..device.block_count() {
            if device.read_block(idx, &mut block).is_err() {
                eprintln!("fsck-statefs: {path}: repaired image read failed");
                return ExitCode::from(2);
            }
            out.extend_from_slice(&block);
        }
        if let Err(err) = std::fs::write(&path, &out) {
            eprintln!("fsck-statefs: write {path}: {err}");
            return ExitCode::from(2);
        }
    }

    match report.outcome {
        FsckOutcome::Clean => ExitCode::SUCCESS,
        FsckOutcome::Repaired => ExitCode::from(1),
        FsckOutcome::Unrecoverable => ExitCode::from(2),
    }
}
