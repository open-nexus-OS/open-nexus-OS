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

use statefs::enc::{EncContext, RecordKey, SALT_LEN};
use statefs::{fsck_with_enc, FsckOutcome, JournalLayout, FSCK_BLOCK_SIZE};
use storage::{BlockDevice, MemBlockDevice};

fn usage() -> ExitCode {
    eprintln!(
        "usage: fsck-statefs [--repair] [--dry-run] \
         [--enc-key-hex <64hex> --enc-class <name> --enc-salt-hex <24hex>] <journal-image>"
    );
    ExitCode::from(2)
}

/// Decode a fixed-length hex CLI argument.
fn hex_bytes<const N: usize>(hex: &str) -> Option<[u8; N]> {
    if hex.len() != N * 2 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; N];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let s = core::str::from_utf8(chunk).ok()?;
        out[i] = u8::from_str_radix(s, 16).ok()?;
    }
    Some(out)
}

fn main() -> ExitCode {
    let mut repair = false;
    let mut dry_run = false;
    let mut image_path: Option<String> = None;
    let mut enc_key_hex: Option<String> = None;
    let mut enc_class: Option<String> = None;
    let mut enc_salt_hex: Option<String> = None;
    let mut pending: Option<&'static str> = None;
    for arg in std::env::args().skip(1) {
        if let Some(flag) = pending.take() {
            match flag {
                "key" => enc_key_hex = Some(arg),
                "class" => enc_class = Some(arg),
                _ => enc_salt_hex = Some(arg),
            }
            continue;
        }
        match arg.as_str() {
            "--repair" => repair = true,
            "--dry-run" => dry_run = true,
            "--enc-key-hex" => pending = Some("key"),
            "--enc-class" => pending = Some("class"),
            "--enc-salt-hex" => pending = Some("salt"),
            other if image_path.is_none() && !other.starts_with('-') => {
                image_path = Some(other.to_string());
            }
            _ => return usage(),
        }
    }
    let Some(path) = image_path else {
        return usage();
    };
    if pending.is_some() {
        return usage();
    }
    // TASK-0027: an explicit key + class + salt build the verification
    // context (offline tooling never derives keys). All three or none.
    let enc_ctx = match (&enc_key_hex, &enc_class, &enc_salt_hex) {
        (None, None, None) => None,
        (Some(key_hex), Some(class), Some(salt_hex)) => {
            let (Some(key), Some(salt)) =
                (hex_bytes::<32>(key_hex), hex_bytes::<SALT_LEN>(salt_hex))
            else {
                return usage();
            };
            let class: &'static str = Box::leak(class.clone().into_boxed_str());
            let mut ctx = EncContext::new(salt);
            let Ok(idx) = ctx.add_class(class, &RecordKey::from_bytes(key)) else {
                return usage();
            };
            let _ = idx;
            Some(ctx)
        }
        _ => return usage(),
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
    let (report, device) = fsck_with_enc(device, apply_repair, enc_ctx.as_ref());

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
    if report.enc_records > 0 {
        if enc_ctx.is_some() {
            println!(
                "fsck-statefs: {path}: enc: {} sealed values, {} FAILED AEAD verification",
                report.enc_records, report.enc_failures
            );
        } else {
            println!(
                "fsck-statefs: {path}: enc: {} sealed values (no key provided, not verified)",
                report.enc_records
            );
        }
    }
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
        // Decrypt failures are report-only (ciphertext is never rewritten;
        // a keyed replay discards the affected txns) but never exit 0.
        FsckOutcome::Clean if report.enc_failures > 0 => ExitCode::from(1),
        FsckOutcome::Clean => ExitCode::SUCCESS,
        FsckOutcome::Repaired => ExitCode::from(1),
        FsckOutcome::Unrecoverable => ExitCode::from(2),
    }
}
