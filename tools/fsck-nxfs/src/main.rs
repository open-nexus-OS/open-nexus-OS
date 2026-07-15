// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: fsck-nxfs CLI — offline validation/repair of nxfs container
//! images in host workflows (RFC-0071 / TASK-0292). Deterministic report,
//! stable exit codes (0 clean / 1 repaired-or-orphan / 2 unrecoverable).
//! Repair never invents data: it retires journal tails replay already
//! proved uncommitted.
//! OWNERS: @runtime
//! STATUS: Functional
//! TEST_COVERAGE: integration test below (tests/cli.rs) + nxfs crash suite

use std::process::ExitCode;

use nxfs::{fsck, FsckOutcome, LOGICAL_BLOCK_SIZE};
use storage::{BlockDevice, MemBlockDevice};

fn usage() -> ExitCode {
    eprintln!("usage: fsck-nxfs [--repair] <container-image>");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let mut repair = false;
    let mut image_path: Option<String> = None;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--repair" => repair = true,
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
            eprintln!("fsck-nxfs: read {path}: {err}");
            return ExitCode::from(2);
        }
    };
    if bytes.is_empty() || bytes.len() % LOGICAL_BLOCK_SIZE != 0 {
        eprintln!(
            "fsck-nxfs: {path}: image length {} is not a multiple of the {LOGICAL_BLOCK_SIZE}-byte logical block",
            bytes.len()
        );
        return ExitCode::from(2);
    }

    let block_count = (bytes.len() / LOGICAL_BLOCK_SIZE) as u64;
    let mut device = MemBlockDevice::new(LOGICAL_BLOCK_SIZE, block_count);
    for (idx, chunk) in bytes.chunks(LOGICAL_BLOCK_SIZE).enumerate() {
        if device.write_block(idx as u64, chunk).is_err() {
            eprintln!("fsck-nxfs: {path}: image load failed");
            return ExitCode::from(2);
        }
    }

    let (report, device) = fsck(device, repair);
    println!(
        "fsck-nxfs: {path}: outcome={:?} orphan_tail={} repaired={}",
        report.outcome, report.orphan_tail, report.repaired
    );

    if report.repaired {
        let Some(device) = device else {
            eprintln!("fsck-nxfs: {path}: repaired image unavailable");
            return ExitCode::from(2);
        };
        let mut out = Vec::with_capacity(bytes.len());
        let mut block = vec![0u8; LOGICAL_BLOCK_SIZE];
        for idx in 0..device.block_count() {
            if device.read_block(idx, &mut block).is_err() {
                eprintln!("fsck-nxfs: {path}: repaired image read failed");
                return ExitCode::from(2);
            }
            out.extend_from_slice(&block);
        }
        if let Err(err) = std::fs::write(&path, &out) {
            eprintln!("fsck-nxfs: write {path}: {err}");
            return ExitCode::from(2);
        }
    }

    match report.outcome {
        FsckOutcome::Clean => ExitCode::SUCCESS,
        FsckOutcome::Repaired => ExitCode::from(1),
        FsckOutcome::Unrecoverable => ExitCode::from(2),
    }
}
