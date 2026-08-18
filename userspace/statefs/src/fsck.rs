// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Offline fsck for statefs journals (TASK-0026). Validates both
//! layouts — legacy v1 (records from block 0, whole device) and v2 (`NXS2`
//! superblock + A/B regions + generation) — covering record framing/CRC,
//! superblock sanity, checkpoint/superblock consistency (gen/entries), txn
//! completeness and the zeroed-tail discipline. Detects orphaned
//! transactions (PREPARE/PAYLOAD without COMMIT/ABORT); with `repair` they
//! are retired by APPENDING `TXN_ABORT` records — repair never rewrites
//! committed data, and only re-validated-clean repairs count as repaired.
//! Mirrors the nxfs fsck discipline (userspace/nxfs/src/fsck.rs): the core
//! lives in the engine crate (no_std-compatible, alloc only) and
//! tools/fsck-statefs is the thin host CLI.
//! OWNERS: @runtime
//! STATUS: Functional (host-first)
//! API_STABILITY: Unstable (v2a)
//! TEST_COVERAGE: Unit tests below + tests/fsck.rs (outcome matrix) +
//! tools/fsck-statefs/tests/cli.rs (CLI + exit-code contract)

use alloc::vec;
use alloc::vec::Vec;

use storage::BlockDevice;

use crate::compact::{parse_superblock, region_geometry, SUPERBLOCK_MAGIC};
use crate::journal_v2::{encode_abort, parse_record, JournalOpCode, JournalRecord, MAX_OPEN_TXNS};
use crate::{JournalEngine, RECORD_HEADER_SIZE};

/// Block size the CLI loads journal images at (the 512-byte virtio-blk
/// sector size every statefs journal in this repo rides on).
pub const FSCK_BLOCK_SIZE: usize = 512;

/// Journal layout detected by fsck (docs/storage/statefs.md §Journal v2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalLayout {
    /// Legacy: records from block 0 over the whole device (generation 0).
    V1,
    /// Post-compaction: block 0 = `NXS2` superblock, A/B record regions.
    V2,
}

/// fsck outcome, mapped to stable exit codes by the CLI tool
/// (0 = clean, 1 = repaired/orphan, 2 = unrecoverable).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsckOutcome {
    /// Replays cleanly, no orphaned transactions.
    Clean,
    /// Orphaned transactions were found; with `repair` they were retired via
    /// appended ABORT records, without `repair` they are only reported.
    Repaired,
    /// A structural violation replay would stop on (or an inconsistent /
    /// invalid superblock). `FsckReport::fault` carries location + reason.
    Unrecoverable,
}

/// Location (absolute device byte offset) + stable reason for an
/// unrecoverable structural violation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FsckFault {
    pub offset: u64,
    pub reason: &'static str,
}

/// Deterministic fsck report. On `Unrecoverable` only `outcome` and `fault`
/// are meaningful; the remaining fields stay at their defaults.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsckReport {
    pub outcome: FsckOutcome,
    pub layout: JournalLayout,
    /// Superblock generation (0 for legacy v1 journals).
    pub generation: u32,
    /// Records scanned in the live region.
    pub records: usize,
    /// Committed keys after replay.
    pub entries: usize,
    /// Transactions left open at the end of the journal (PREPARE/PAYLOAD
    /// without COMMIT/ABORT), in journal order — the repairable orphans.
    pub orphan_txns: Vec<u64>,
    /// Inert anomalies replay already ignores deterministically (unknown-txn
    /// COMMIT/PAYLOAD/ABORT, poisoned txns, out-of-place CHECKPOINT records).
    /// Informational: not repairable append-only, never changes the outcome.
    pub anomalies: usize,
    /// Nonzero bytes between the write head and the end of the following
    /// block (crash residue; replay ignores it). Informational.
    pub tail_dirty: bool,
    /// True only when `repair` appended ABORTs AND the re-validation of the
    /// repaired journal came back clean.
    pub repaired: bool,
    /// Sealed (TASK-0027 `NXR1`) values seen in the live region.
    pub enc_records: usize,
    /// Sealed values whose AEAD open FAILED under the provided context
    /// (`fsck_with_enc`). Report-only for committed data: ciphertext is
    /// never rewritten — a keyed replay discards the affected txns; an
    /// uncommitted bad txn falls under the normal orphan repair. Any
    /// nonzero count keeps the CLI exit code away from 0.
    pub enc_failures: usize,
    pub fault: Option<FsckFault>,
}

/// Validates the journal on `device`. With `repair`, orphaned transactions
/// are retired by appending one `TXN_ABORT` per orphan (append-only —
/// committed data is never rewritten), then the journal is re-validated:
/// `repaired` is set only if that re-validation is clean, otherwise the
/// outcome degrades to `Unrecoverable`. The device is returned for
/// write-back except on a pre-repair unrecoverable fault (mirrors nxfs).
pub fn fsck<D: BlockDevice>(device: D, repair: bool) -> (FsckReport, Option<D>) {
    fsck_with_enc(device, repair, None)
}

/// `fsck` with an optional record-encryption context (TASK-0027): sealed
/// values are AEAD-verified during the walk (header-driven — the class
/// index and nonce inputs come from the sealed header, the key path from
/// the record). Without a context sealed values are counted but not
/// verified.
pub fn fsck_with_enc<D: BlockDevice>(
    device: D,
    repair: bool,
    enc: Option<&crate::enc::EncContext>,
) -> (FsckReport, Option<D>) {
    let scan = match scan_device(&device, enc) {
        Ok(scan) => scan,
        Err(f) => return (unrecoverable(f), None),
    };
    let mut engine = match JournalEngine::open(device) {
        Ok(engine) => engine,
        Err(_) => return (unrecoverable(fault(0, "journal replay failed")), None),
    };
    // Cross-check: the validator and the engine replayer must agree on how
    // many records the live region holds.
    if engine.replayed_records() != scan.records {
        return (unrecoverable(fault(0, "validator/replayer divergence")), None);
    }
    let inert = engine.replay_orphans().saturating_sub(scan.orphan_txns.len());
    let mut report = FsckReport {
        outcome: FsckOutcome::Clean,
        layout: scan.layout,
        generation: scan.generation,
        records: scan.records,
        entries: engine.len(),
        orphan_txns: scan.orphan_txns,
        anomalies: inert + scan.checkpoint_anomalies,
        tail_dirty: scan.tail_dirty,
        repaired: false,
        enc_records: scan.enc_records,
        enc_failures: scan.enc_failures,
        fault: None,
    };
    if report.orphan_txns.is_empty() {
        return (report, Some(engine.into_device()));
    }
    report.outcome = FsckOutcome::Repaired;
    if !repair {
        return (report, Some(engine.into_device()));
    }
    // Append-only repair: one ABORT per orphan, in journal (PREPARE) order.
    for &id in &report.orphan_txns {
        if engine.append_bytes(&encode_abort(id)).is_err() {
            report.outcome = FsckOutcome::Unrecoverable;
            report.fault = Some(fault(0, "repair append failed"));
            return (report, Some(engine.into_device()));
        }
    }
    if engine.sync().is_err() {
        report.outcome = FsckOutcome::Unrecoverable;
        report.fault = Some(fault(0, "repair sync failed"));
        return (report, Some(engine.into_device()));
    }
    let device = engine.into_device();
    match scan_device(&device, None) {
        Ok(rescan) if rescan.orphan_txns.is_empty() => {
            report.repaired = true;
            (report, Some(device))
        }
        _ => {
            report.outcome = FsckOutcome::Unrecoverable;
            report.fault = Some(fault(0, "post-repair validation failed"));
            (report, Some(device))
        }
    }
}

// ============================================================================
// Read-only validation scan
// ============================================================================

struct Scan {
    layout: JournalLayout,
    generation: u32,
    records: usize,
    orphan_txns: Vec<u64>,
    checkpoint_anomalies: usize,
    tail_dirty: bool,
    enc_records: usize,
    enc_failures: usize,
}

fn fault(offset: u64, reason: &'static str) -> FsckFault {
    FsckFault { offset, reason }
}

fn unrecoverable(f: FsckFault) -> FsckReport {
    FsckReport {
        outcome: FsckOutcome::Unrecoverable,
        layout: JournalLayout::V1,
        generation: 0,
        records: 0,
        entries: 0,
        orphan_txns: Vec::new(),
        anomalies: 0,
        tail_dirty: false,
        repaired: false,
        enc_records: 0,
        enc_failures: 0,
        fault: Some(f),
    }
}

/// Read-only structural walk of the live journal region. `Err` = a fault
/// replay would stop on (structural corruption, invalid/inconsistent
/// superblock or checkpoint, replay limit).
fn scan_device<D: BlockDevice>(
    device: &D,
    enc: Option<&crate::enc::EncContext>,
) -> Result<Scan, FsckFault> {
    let block_size = device.block_size();
    let block_count = device.block_count();
    if block_size == 0 {
        return Err(fault(0, "invalid block size"));
    }

    // Layout detection (block 0): `NXS2` magic selects the v2 A/B layout;
    // a present-but-invalid superblock is unrecoverable (a legacy replay of
    // superblock bytes would silently discard the whole store).
    let mut layout = JournalLayout::V1;
    let mut generation = 0u32;
    let mut sb_entries = 0u32;
    let mut region_first = 0u64;
    let mut region_blocks = block_count;
    if block_size >= SUPERBLOCK_MAGIC.len() && block_count > 0 {
        let mut b0 = vec![0u8; block_size];
        device.read_block(0, &mut b0).map_err(|_| fault(0, "device read failed"))?;
        if b0[..4] == SUPERBLOCK_MAGIC {
            let Some((active, gen, entries)) = parse_superblock(&b0) else {
                return Err(fault(0, "invalid superblock"));
            };
            let Some((blocks, a_first, b_first)) = region_geometry(block_count) else {
                return Err(fault(0, "device too small for v2 layout"));
            };
            layout = JournalLayout::V2;
            generation = gen;
            sb_entries = entries;
            region_first = if active == 0 { a_first } else { b_first };
            region_blocks = blocks;
        }
    }
    let region_base = region_first.saturating_mul(block_size as u64);

    // Load the live region (bounded by device size; offline tool profile).
    let mut region = vec![0u8; (region_blocks as usize).saturating_mul(block_size)];
    let mut block_buf = vec![0u8; block_size];
    for i in 0..region_blocks {
        device.read_block(region_first + i, &mut block_buf).map_err(|_| {
            fault((region_first + i).saturating_mul(block_size as u64), "device read failed")
        })?;
        let start = i as usize * block_size;
        region[start..start + block_size].copy_from_slice(&block_buf);
    }

    // Record walk: framing/CRC via `parse_record`, checkpoint placement,
    // and a mirror of the replay TxnTable id-membership rules.
    let mut pos = 0usize;
    let mut records = 0usize;
    let mut open: Vec<u64> = Vec::new();
    let mut checkpoint_anomalies = 0usize;
    let mut enc_records = 0usize;
    let mut enc_failures = 0usize;
    loop {
        if records >= crate::MAX_REPLAY_RECORDS {
            return Err(fault(region_base + pos as u64, "replay record limit exceeded"));
        }
        match parse_record(&region[pos..]) {
            Ok(Some((record, consumed))) => {
                check_checkpoint(layout, records, &record, generation, sb_entries)
                    .map_err(|reason| fault(region_base + pos as u64, reason))?;
                if record.op == JournalOpCode::Checkpoint
                    && !(layout == JournalLayout::V2 && records == 0)
                {
                    // Replay tolerates it (state reset) but only compaction
                    // legitimately writes CHECKPOINT — report it.
                    checkpoint_anomalies += 1;
                }
                // TASK-0027: sealed values are counted; with a context they
                // are AEAD-verified (Put value directly, txn chunk past the
                // 8-byte id prefix). Failures are reported, never repaired
                // by rewriting — a keyed replay discards the affected txns.
                let sealed_view = match record.op {
                    JournalOpCode::Put => Some(record.value.as_slice()),
                    JournalOpCode::TxnPayload if record.value.len() >= 8 => {
                        Some(&record.value[8..])
                    }
                    _ => None,
                };
                if let Some(bytes) = sealed_view {
                    if crate::enc::is_sealed(bytes) {
                        enc_records += 1;
                        if let Some(ctx) = enc {
                            if !crate::enc::verify(ctx, &record.key, bytes) {
                                enc_failures += 1;
                            }
                        }
                    }
                }
                track_open_txns(&mut open, &record);
                pos += consumed;
                records += 1;
            }
            // Clean end: magic mismatch or truncated (torn) tail.
            Ok(None) => break,
            Err(_) => {
                // Corruption followed by a valid record = mid-journal damage
                // replay would stop on, silently losing that record: fatal.
                // Corruption with nothing valid after it is torn-tail crash
                // residue replay already discards: reported via tail_dirty.
                if has_valid_record_after(&region, pos) {
                    return Err(fault(region_base + pos as u64, classify(&region[pos..])));
                }
                break;
            }
        }
    }
    if layout == JournalLayout::V2 && records == 0 {
        return Err(fault(region_base, "missing checkpoint at snapshot start"));
    }

    // Zeroed-tail discipline: bytes past the write head through the end of
    // the FOLLOWING block must be zero. Nonzero = crash residue replay
    // already ignores — reported, not fatal.
    let tail_end = core::cmp::min((pos / block_size + 2).saturating_mul(block_size), region.len());
    let tail_dirty = region[pos..tail_end].iter().any(|&b| b != 0);

    Ok(Scan {
        layout,
        generation,
        records,
        orphan_txns: open,
        checkpoint_anomalies,
        tail_dirty,
        enc_records,
        enc_failures,
    })
}

/// v2 snapshot rule: the live region MUST start with `CHECKPOINT{gen,
/// entries}` matching the superblock (both are written by one compaction
/// cycle; disagreement means metadata the flip discipline excludes).
fn check_checkpoint(
    layout: JournalLayout,
    records: usize,
    record: &JournalRecord,
    sb_generation: u32,
    sb_entries: u32,
) -> Result<(), &'static str> {
    if layout != JournalLayout::V2 || records > 0 {
        return Ok(());
    }
    if record.op != JournalOpCode::Checkpoint {
        return Err("missing checkpoint at snapshot start");
    }
    if read_u32_le(&record.value) != sb_generation {
        return Err("checkpoint/superblock generation mismatch");
    }
    if read_u32_le(record.value.get(4..).unwrap_or(&[])) != sb_entries {
        return Err("checkpoint/superblock entry-count mismatch");
    }
    Ok(())
}

/// Mirror of the replay `TxnTable` id-membership rules (journal_v2.rs):
/// fsck must agree with replay about which transactions are still open.
/// Poisoned txns stay open (their PREPARE lacks a COMMIT/ABORT); a
/// PREPARE past the open-txn cap stays untracked (its records are inert).
fn track_open_txns(open: &mut Vec<u64>, record: &JournalRecord) {
    match record.op {
        JournalOpCode::TxnPrepare => {
            let id = read_u64_le(&record.value);
            // Re-PREPARE of an open id keeps it open (replay re-begins it).
            if !open.contains(&id) && open.len() < MAX_OPEN_TXNS {
                open.push(id);
            }
        }
        JournalOpCode::TxnCommit | JournalOpCode::TxnAbort => {
            let id = read_u64_le(&record.value);
            open.retain(|&x| x != id);
        }
        // Snapshot boundary: replay clears the txn table.
        JournalOpCode::Checkpoint => open.clear(),
        _ => {}
    }
}

/// True if any position after `from` parses as a valid record — the
/// distinction between mid-journal corruption (fatal: replay would stop
/// before valid data) and discarded torn-tail residue (reported only).
/// Conservative: value bytes that happen to embed a whole valid record
/// count as "valid data after" (errs toward Unrecoverable). Bounded by the
/// region size.
fn has_valid_record_after(region: &[u8], from: usize) -> bool {
    let magic = crate::JOURNAL_MAGIC.to_le_bytes();
    let mut i = from + 1;
    while i + RECORD_HEADER_SIZE <= region.len() {
        if region[i..i + 4] == magic {
            if let Ok(Some(_)) = parse_record(&region[i..]) {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// Stable reason for a record `parse_record` rejects (it only reports
/// `Corrupted`). Derived by elimination: opcode, then length caps, then
/// CRC, then key encoding — a record passing all of those can only have
/// been rejected for its per-opcode payload shape.
fn classify(data: &[u8]) -> &'static str {
    if data.len() < RECORD_HEADER_SIZE {
        return "truncated record header";
    }
    if JournalOpCode::from_u8(data[4]).is_none() {
        return "unknown opcode";
    }
    let key_len = u16::from_le_bytes([data[5], data[6]]) as usize;
    let value_len = u32::from_le_bytes([data[7], data[8], data[9], data[10]]) as usize;
    if key_len > crate::MAX_KEY_LEN || value_len > crate::MAX_VALUE_SIZE {
        return "record length exceeds caps";
    }
    let total = RECORD_HEADER_SIZE + key_len + value_len;
    if data.len() < total {
        // A shape-invalid record cut short by the region end; parse_record
        // rejects it before the torn-tail check can accept it.
        return "truncated record";
    }
    let crc_at = total - 4;
    let stored =
        u32::from_le_bytes([data[crc_at], data[crc_at + 1], data[crc_at + 2], data[crc_at + 3]]);
    if crate::crc32c(&data[..crc_at]) != stored {
        return "crc mismatch";
    }
    if core::str::from_utf8(&data[11..11 + key_len]).is_err() {
        return "invalid key encoding";
    }
    "invalid record shape"
}

fn read_u64_le(value: &[u8]) -> u64 {
    let mut buf = [0u8; 8];
    let len = core::cmp::min(value.len(), 8);
    buf[..len].copy_from_slice(&value[..len]);
    u64::from_le_bytes(buf)
}

fn read_u32_le(value: &[u8]) -> u32 {
    let mut buf = [0u8; 4];
    let len = core::cmp::min(value.len(), 4);
    buf[..len].copy_from_slice(&value[..len]);
    u32::from_le_bytes(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal_v2::encode_record;
    use storage::MemBlockDevice;

    /// A compacted (v2, generation 1) image with two committed keys.
    fn v2_image() -> MemBlockDevice {
        let mut engine = JournalEngine::open(MemBlockDevice::new(512, 64)).expect("open");
        engine.put("/state/test/a", b"alpha").expect("put");
        engine.put("/state/test/b", b"beta").expect("put");
        engine.compact_now().expect("compact");
        engine.into_device()
    }

    #[test]
    fn classify_reasons_are_stable() {
        // Unknown opcode wins over the (now stale) CRC.
        let mut bytes = encode_record(JournalOpCode::Put, "/state/t/k", b"v");
        bytes[4] = 0xFF;
        assert!(parse_record(&bytes).is_err());
        assert_eq!(classify(&bytes), "unknown opcode");

        // Valid CRC, invalid per-opcode shape.
        let bad_shape = encode_record(JournalOpCode::TxnPrepare, "", &[1, 2, 3, 4]);
        assert!(parse_record(&bad_shape).is_err());
        assert_eq!(classify(&bad_shape), "invalid record shape");

        // Damaged payload byte: CRC mismatch.
        let mut damaged = encode_record(JournalOpCode::Put, "/state/t/k", b"value");
        let flip = damaged.len() - 6; // inside the value, before the CRC
        damaged[flip] ^= 0xFF;
        assert!(parse_record(&damaged).is_err());
        assert_eq!(classify(&damaged), "crc mismatch");
    }

    #[test]
    fn test_reject_superblock_generation_mismatch() {
        let mut device = v2_image();
        // Flip the generation and RE-SEAL the CRC: the superblock is valid
        // on its own but disagrees with the checkpoint record.
        let block = &mut device.raw_storage_mut()[0];
        block[8] ^= 0x01;
        let crc = crate::crc32c(&block[..16]);
        block[16..20].copy_from_slice(&crc.to_le_bytes());
        let (report, device) = fsck(device, false);
        assert_eq!(report.outcome, FsckOutcome::Unrecoverable);
        let f = report.fault.expect("fault");
        assert_eq!(f.reason, "checkpoint/superblock generation mismatch");
        assert!(device.is_none());
    }

    #[test]
    fn test_reject_superblock_entry_count_mismatch() {
        let mut device = v2_image();
        let block = &mut device.raw_storage_mut()[0];
        block[12] ^= 0x01;
        let crc = crate::crc32c(&block[..16]);
        block[16..20].copy_from_slice(&crc.to_le_bytes());
        let (report, _) = fsck(device, false);
        assert_eq!(report.outcome, FsckOutcome::Unrecoverable);
        let f = report.fault.expect("fault");
        assert_eq!(f.reason, "checkpoint/superblock entry-count mismatch");
    }

    #[test]
    fn test_reject_damaged_superblock_is_unrecoverable() {
        let mut device = v2_image();
        // Damage past the magic WITHOUT re-sealing: superblock CRC fails.
        device.raw_storage_mut()[0][8] ^= 0x01;
        let (report, device) = fsck(device, false);
        assert_eq!(report.outcome, FsckOutcome::Unrecoverable);
        let f = report.fault.expect("fault");
        assert_eq!(f.offset, 0);
        assert_eq!(f.reason, "invalid superblock");
        assert!(device.is_none());
    }
}
