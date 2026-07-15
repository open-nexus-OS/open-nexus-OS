// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: nxfs bounded metadata journal (RFC-0071): linear byte-run of
//! crc32c-framed records inside a fixed region, reset by every checkpoint.
//! Replay applies COMMITTED transactions only and stops deterministically at
//! the first invalid/torn record — uncommitted tails are discarded, never
//! half-applied.
//! OWNERS: @runtime
//! STATUS: Experimental (TASK-0292)
//! TEST_COVERAGE: record roundtrip + torn-tail tests below; end-to-end crash
//! injection in tests/crash_injection.rs

use alloc::string::String;
use alloc::vec::Vec;

use crate::format::{crc32c, JOURNAL_MAGIC, MAX_NAME_LEN};
use crate::state::Extent;
use crate::{NxfsError, Result};

const KIND_BEGIN: u8 = 1;
const KIND_COMMIT: u8 = 2;
const KIND_MKNODE: u8 = 3;
const KIND_WRITE: u8 = 4;
const KIND_REMOVE: u8 = 5;
const KIND_RENAME: u8 = 6;

/// Fixed per-record overhead: magic(4) + txn(8) + kind(1) + len(4) + crc(4).
const RECORD_OVERHEAD: usize = 4 + 8 + 1 + 4 + 4;
/// Bounded op payload (rename carries two names).
const MAX_PAYLOAD: usize = 64 + 2 * (MAX_NAME_LEN + 2);
/// Bounded extent count per Write op (continuation via multiple Writes).
pub(crate) const MAX_EXTENTS_PER_WRITE: usize = 256;

/// One journaled mutation. `apply` semantics live in `state::State::apply`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Op {
    MkNode { parent: u64, id: u64, kind: u8, name: String },
    Write { id: u64, size: u64, extents: Vec<Extent> },
    Remove { parent: u64, id: u64, name: String },
    Rename {
        from_parent: u64,
        from_name: String,
        to_parent: u64,
        to_name: String,
        replaced: u64,
    },
}

/// Encodes one full transaction (`BEGIN ops… COMMIT`) as a contiguous byte
/// run. A crash anywhere inside the run leaves an incomplete group that
/// replay discards wholesale.
pub(crate) fn encode_txn(txn: u64, ops: &[Op]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    push_record(&mut out, txn, KIND_BEGIN, &[]);
    for op in ops {
        let (kind, payload) = encode_op(op)?;
        push_record(&mut out, txn, kind, &payload);
    }
    push_record(&mut out, txn, KIND_COMMIT, &[]);
    Ok(out)
}

fn push_record(out: &mut Vec<u8>, txn: u64, kind: u8, payload: &[u8]) {
    let start = out.len();
    out.extend_from_slice(&JOURNAL_MAGIC);
    out.extend_from_slice(&txn.to_le_bytes());
    out.push(kind);
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    let crc = crc32c(&out[start..]);
    out.extend_from_slice(&crc.to_le_bytes());
}

fn encode_op(op: &Op) -> Result<(u8, Vec<u8>)> {
    let mut payload = Vec::new();
    match op {
        Op::MkNode { parent, id, kind, name } => {
            push_name_bounds(name)?;
            payload.extend_from_slice(&parent.to_le_bytes());
            payload.extend_from_slice(&id.to_le_bytes());
            payload.push(*kind);
            push_name(&mut payload, name);
            Ok((KIND_MKNODE, payload))
        }
        Op::Write { id, size, extents } => {
            if extents.len() > MAX_EXTENTS_PER_WRITE {
                return Err(NxfsError::TooBig);
            }
            payload.extend_from_slice(&id.to_le_bytes());
            payload.extend_from_slice(&size.to_le_bytes());
            payload.extend_from_slice(&(extents.len() as u16).to_le_bytes());
            for extent in extents {
                payload.extend_from_slice(&extent.lb.to_le_bytes());
                payload.extend_from_slice(&extent.blocks.to_le_bytes());
            }
            Ok((KIND_WRITE, payload))
        }
        Op::Remove { parent, id, name } => {
            push_name_bounds(name)?;
            payload.extend_from_slice(&parent.to_le_bytes());
            payload.extend_from_slice(&id.to_le_bytes());
            push_name(&mut payload, name);
            Ok((KIND_REMOVE, payload))
        }
        Op::Rename { from_parent, from_name, to_parent, to_name, replaced } => {
            push_name_bounds(from_name)?;
            push_name_bounds(to_name)?;
            payload.extend_from_slice(&from_parent.to_le_bytes());
            payload.extend_from_slice(&to_parent.to_le_bytes());
            payload.extend_from_slice(&replaced.to_le_bytes());
            push_name(&mut payload, from_name);
            push_name(&mut payload, to_name);
            Ok((KIND_RENAME, payload))
        }
    }
}

fn push_name_bounds(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > MAX_NAME_LEN {
        return Err(NxfsError::TooBig);
    }
    Ok(())
}

fn push_name(payload: &mut Vec<u8>, name: &str) {
    payload.push(name.len() as u8);
    payload.extend_from_slice(name.as_bytes());
}

/// Replays a journal byte region: returns the committed transactions' ops in
/// order plus the byte offset just past the last COMPLETE record (the write
/// head for new appends). Stops at the first invalid/torn record; an
/// incomplete trailing transaction is discarded wholesale.
///
/// `min_txn` is the checkpoint's `next_txn` watermark: complete groups with
/// `txn < min_txn` are STALE leftovers from before that checkpoint — they are
/// skipped (never re-applied onto newer state) but still advance the write
/// head. This makes the checkpoint flip crash-safe without a journal-zeroing
/// window.
pub(crate) fn replay(region: &[u8], min_txn: u64) -> ReplayResult {
    let mut committed: Vec<Op> = Vec::new();
    let mut pending: Vec<Op> = Vec::new();
    let mut pending_txn: Option<u64> = None;
    let mut offset = 0usize;
    let mut committed_end = 0usize;
    let mut max_seen_txn = 0u64;
    let mut orphan = false;

    while offset + RECORD_OVERHEAD <= region.len() {
        let Some((txn, kind, payload, next)) = parse_record(region, offset) else {
            break;
        };
        offset = next;
        // Track the highest id SEEN (committed or torn): fresh transactions
        // must never reuse a discarded id, or the stale-skip watermark could
        // misclassify them after a repair.
        if txn > max_seen_txn {
            max_seen_txn = txn;
        }
        // Stale groups (txn < min_txn) are pre-checkpoint leftovers: they are
        // parsed for framing but never applied, and their anomalies (torn
        // tails, poisoned records) are NOT orphans — the checkpoint already
        // superseded them.
        let live = txn >= min_txn;
        match kind {
            KIND_BEGIN => {
                // A BEGIN while a txn is open orphans the open one.
                orphan |= pending_txn.is_some_and(|open| open >= min_txn);
                pending_txn = Some(txn);
                pending.clear();
            }
            KIND_COMMIT => {
                if pending_txn == Some(txn) {
                    if live {
                        committed.append(&mut pending);
                    }
                    pending.clear();
                    pending_txn = None;
                    committed_end = offset;
                } else {
                    orphan |= live;
                    pending_txn = None;
                    pending.clear();
                }
            }
            _ => {
                if pending_txn == Some(txn) {
                    match decode_op(kind, payload) {
                        Some(op) => pending.push(op),
                        None => {
                            // Malformed op inside a group: poison the group.
                            orphan |= live;
                            pending_txn = None;
                            pending.clear();
                        }
                    }
                } else {
                    orphan |= live;
                }
            }
        }
    }
    orphan |= pending_txn.is_some_and(|open| open >= min_txn);
    let next_txn = core::cmp::max(max_seen_txn + 1, min_txn);
    ReplayResult { ops: committed, write_head: committed_end, next_txn, orphan }
}

/// Outcome of a journal replay.
pub(crate) struct ReplayResult {
    pub ops: Vec<Op>,
    /// Byte offset for the next append (just past the last committed txn).
    pub write_head: usize,
    pub next_txn: u64,
    /// True when a torn/orphaned transaction tail was discarded.
    pub orphan: bool,
}

fn parse_record(region: &[u8], offset: usize) -> Option<(u64, u8, &[u8], usize)> {
    let head = &region[offset..];
    if head[0..4] != JOURNAL_MAGIC {
        return None;
    }
    let txn = u64::from_le_bytes(head[4..12].try_into().ok()?);
    let kind = head[12];
    let len = u32::from_le_bytes(head[13..17].try_into().ok()?) as usize;
    if len > MAX_PAYLOAD + MAX_EXTENTS_PER_WRITE * 12 {
        return None;
    }
    let total = RECORD_OVERHEAD + len;
    if offset + total > region.len() {
        return None;
    }
    let payload = &region[offset + 17..offset + 17 + len];
    let stored = u32::from_le_bytes(region[offset + 17 + len..offset + total].try_into().ok()?);
    if crc32c(&region[offset..offset + 17 + len]) != stored {
        return None;
    }
    Some((txn, kind, payload, offset + total))
}

fn decode_op(kind: u8, payload: &[u8]) -> Option<Op> {
    let mut off = 0usize;
    match kind {
        KIND_MKNODE => {
            let parent = take_u64(payload, &mut off)?;
            let id = take_u64(payload, &mut off)?;
            let node_kind = *payload.get(off)?;
            off += 1;
            let name = take_name(payload, &mut off)?;
            (off == payload.len()).then_some(Op::MkNode { parent, id, kind: node_kind, name })
        }
        KIND_WRITE => {
            let id = take_u64(payload, &mut off)?;
            let size = take_u64(payload, &mut off)?;
            let count = take_u16(payload, &mut off)? as usize;
            if count > MAX_EXTENTS_PER_WRITE {
                return None;
            }
            let mut extents = Vec::with_capacity(count);
            for _ in 0..count {
                let lb = take_u64(payload, &mut off)?;
                let blocks = take_u32(payload, &mut off)?;
                extents.push(Extent { lb, blocks });
            }
            (off == payload.len()).then_some(Op::Write { id, size, extents })
        }
        KIND_REMOVE => {
            let parent = take_u64(payload, &mut off)?;
            let id = take_u64(payload, &mut off)?;
            let name = take_name(payload, &mut off)?;
            (off == payload.len()).then_some(Op::Remove { parent, id, name })
        }
        KIND_RENAME => {
            let from_parent = take_u64(payload, &mut off)?;
            let to_parent = take_u64(payload, &mut off)?;
            let replaced = take_u64(payload, &mut off)?;
            let from_name = take_name(payload, &mut off)?;
            let to_name = take_name(payload, &mut off)?;
            (off == payload.len()).then_some(Op::Rename {
                from_parent,
                from_name,
                to_parent,
                to_name,
                replaced,
            })
        }
        _ => None,
    }
}

fn take_u64(buf: &[u8], off: &mut usize) -> Option<u64> {
    let bytes = buf.get(*off..*off + 8)?;
    *off += 8;
    Some(u64::from_le_bytes(bytes.try_into().ok()?))
}

fn take_u32(buf: &[u8], off: &mut usize) -> Option<u32> {
    let bytes = buf.get(*off..*off + 4)?;
    *off += 4;
    Some(u32::from_le_bytes(bytes.try_into().ok()?))
}

fn take_u16(buf: &[u8], off: &mut usize) -> Option<u16> {
    let bytes = buf.get(*off..*off + 2)?;
    *off += 2;
    Some(u16::from_le_bytes(bytes.try_into().ok()?))
}

fn take_name(buf: &[u8], off: &mut usize) -> Option<String> {
    let len = *buf.get(*off)? as usize;
    *off += 1;
    if len == 0 || len > MAX_NAME_LEN {
        return None;
    }
    let bytes = buf.get(*off..*off + len)?;
    *off += len;
    Some(String::from(core::str::from_utf8(bytes).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn ops() -> Vec<Op> {
        vec![
            Op::MkNode { parent: 1, id: 2, kind: 0, name: "a.txt".into() },
            Op::Write { id: 2, size: 10, extents: vec![Extent { lb: 40, blocks: 1 }] },
            Op::Rename {
                from_parent: 1,
                from_name: "a.txt".into(),
                to_parent: 1,
                to_name: "b.txt".into(),
                replaced: 0,
            },
        ]
    }

    #[test]
    fn txn_roundtrip() {
        let bytes = encode_txn(7, &ops()).expect("encode");
        let result = replay(&bytes, 0);
        assert_eq!(result.ops, ops());
        assert_eq!(result.write_head, bytes.len());
        assert_eq!(result.next_txn, 8);
        assert!(!result.orphan);
    }

    #[test]
    fn torn_tail_is_discarded_at_every_cut() {
        let committed = encode_txn(1, &ops()).expect("encode");
        let torn = encode_txn(2, &ops()).expect("encode");
        for cut in 0..torn.len() {
            let mut region = committed.clone();
            region.extend_from_slice(&torn[..cut]);
            let result = replay(&region, 0);
            assert_eq!(result.ops, ops(), "cut={cut}: only txn 1 applies");
            assert_eq!(result.write_head, committed.len(), "cut={cut}");
        }
        // The complete run applies both.
        let mut region = committed.clone();
        region.extend_from_slice(&torn);
        assert_eq!(replay(&region, 0).ops.len(), ops().len() * 2);
    }

    #[test]
    fn test_reject_corrupt_record_poisons_group() {
        let mut bytes = encode_txn(1, &ops()).expect("encode");
        // Flip one payload byte of the middle record: crc fails → replay
        // stops before COMMIT → nothing applies.
        let mid = bytes.len() / 2;
        bytes[mid] ^= 0x01;
        let result = replay(&bytes, 0);
        assert!(result.ops.is_empty());
        assert_eq!(result.write_head, 0);
    }
}
