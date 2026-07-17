// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: nxfs checkpoint serialization — the object + directory tables
//! frozen into one crc-checked byte blob. Phase 1 writes checkpoints into two
//! FIXED alternating regions (the RFC-0071 checkpoint-flip commit protocol;
//! full CoW trees arrive in Phase 3 without a format break). The allocation
//! bitmap is NOT serialized — it is derived from the loaded extents.
//! OWNERS: @runtime
//! STATUS: Experimental (TASK-0292)
//! TEST_COVERAGE: roundtrip + reject tests below

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use crate::format::{KIND_DIR, MAX_NAME_LEN};
use crate::state::{DirTable, Extent, Object, State};
use crate::{NxfsError, Result};

/// Serializes the state (objects + dirs + next ids) into a checkpoint blob.
pub(crate) fn encode(state: &State, next_txn: u64) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&state.next_object.to_le_bytes());
    out.extend_from_slice(&next_txn.to_le_bytes());
    out.extend_from_slice(&(state.objects.len() as u32).to_le_bytes());
    for (id, object) in &state.objects {
        out.extend_from_slice(&id.to_le_bytes());
        out.push(object.kind);
        out.extend_from_slice(&object.size.to_le_bytes());
        out.extend_from_slice(&(object.extents.len() as u32).to_le_bytes());
        for extent in &object.extents {
            out.extend_from_slice(&extent.lb.to_le_bytes());
            out.extend_from_slice(&extent.blocks.to_le_bytes());
        }
    }
    out.extend_from_slice(&(state.dirs.len() as u32).to_le_bytes());
    for (id, table) in &state.dirs {
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&(table.len() as u32).to_le_bytes());
        for (name, (child, kind)) in table {
            out.push(name.len() as u8);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&child.to_le_bytes());
            out.push(*kind);
        }
    }
    out
}

/// Deserializes a checkpoint blob into fresh state (bitmap derived from the
/// reserved regions + loaded extents). Fail-closed on any structural error.
pub(crate) fn decode(blob: &[u8], total_blocks: u64, data_start: u64) -> Result<(State, u64)> {
    let mut off = 0usize;
    let next_object = take_u64(blob, &mut off)?;
    let next_txn = take_u64(blob, &mut off)?;
    let object_count = take_u32(blob, &mut off)? as usize;

    let mut objects: BTreeMap<u64, Object> = BTreeMap::new();
    for _ in 0..object_count {
        let id = take_u64(blob, &mut off)?;
        let kind = take_u8(blob, &mut off)?;
        let size = take_u64(blob, &mut off)?;
        let extent_count = take_u32(blob, &mut off)? as usize;
        let mut extents = Vec::with_capacity(extent_count.min(1024));
        for _ in 0..extent_count {
            let lb = take_u64(blob, &mut off)?;
            let blocks = take_u32(blob, &mut off)?;
            if lb.saturating_add(u64::from(blocks)) > total_blocks {
                return Err(NxfsError::Integrity);
            }
            extents.push(Extent { lb, blocks });
        }
        objects.insert(id, Object { kind, size, extents });
    }

    let dir_count = take_u32(blob, &mut off)? as usize;
    let mut dirs: BTreeMap<u64, DirTable> = BTreeMap::new();
    for _ in 0..dir_count {
        let id = take_u64(blob, &mut off)?;
        let entry_count = take_u32(blob, &mut off)? as usize;
        let mut table = DirTable::new();
        for _ in 0..entry_count {
            let name_len = take_u8(blob, &mut off)? as usize;
            if name_len == 0 || name_len > MAX_NAME_LEN {
                return Err(NxfsError::Integrity);
            }
            let bytes = blob.get(off..off + name_len).ok_or(NxfsError::Integrity)?;
            off += name_len;
            let name = String::from(core::str::from_utf8(bytes).map_err(|_| NxfsError::Integrity)?);
            let child = take_u64(blob, &mut off)?;
            let kind = take_u8(blob, &mut off)?;
            table.insert(name, (child, kind));
        }
        dirs.insert(id, table);
    }
    if off != blob.len() {
        return Err(NxfsError::Integrity);
    }

    // Structural cross-checks: every dir table has a dir object; every dir
    // entry points at an existing object.
    for (id, table) in &dirs {
        let object = objects.get(id).ok_or(NxfsError::Integrity)?;
        if object.kind != KIND_DIR {
            return Err(NxfsError::Integrity);
        }
        for (child, _) in table.values() {
            if !objects.contains_key(child) {
                return Err(NxfsError::Integrity);
            }
        }
    }

    let mut state = State::new_empty(total_blocks, data_start);
    state.objects = objects;
    state.dirs = dirs;
    state.next_object = next_object;
    // Derive the bitmap from the loaded extents (idempotent marks).
    let all_extents: Vec<Extent> =
        state.objects.values().flat_map(|o| o.extents.iter().copied()).collect();
    state.mark_extents(&all_extents);
    Ok((state, next_txn))
}

fn take_u8(buf: &[u8], off: &mut usize) -> Result<u8> {
    let byte = *buf.get(*off).ok_or(NxfsError::Integrity)?;
    *off += 1;
    Ok(byte)
}

fn take_u32(buf: &[u8], off: &mut usize) -> Result<u32> {
    let bytes = buf.get(*off..*off + 4).ok_or(NxfsError::Integrity)?;
    *off += 4;
    Ok(u32::from_le_bytes(bytes.try_into().map_err(|_| NxfsError::Integrity)?))
}

fn take_u64(buf: &[u8], off: &mut usize) -> Result<u64> {
    let bytes = buf.get(*off..*off + 8).ok_or(NxfsError::Integrity)?;
    *off += 8;
    Ok(u64::from_le_bytes(bytes.try_into().map_err(|_| NxfsError::Integrity)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{KIND_FILE, ROOT_OBJECT};
    use crate::journal::Op;

    #[test]
    fn checkpoint_roundtrip() {
        let mut state = State::new_empty(256, 10);
        state
            .apply(&Op::MkNode { parent: ROOT_OBJECT, id: 2, kind: KIND_FILE, name: "f".into() })
            .expect("mknode");
        let extents = state.alloc_blocks(3).expect("alloc");
        state.apply(&Op::Write { id: 2, size: 9000, extents }).expect("write");

        let blob = encode(&state, 42);
        let (loaded, next_txn) = decode(&blob, 256, 10).expect("decode");
        assert_eq!(next_txn, 42);
        assert_eq!(loaded.objects, state.objects);
        assert_eq!(loaded.dirs, state.dirs);
        assert_eq!(loaded.next_object, state.next_object);
        // Derived bitmap: allocating from the loaded state must not hand out
        // blocks the file already owns.
        let mut loaded = loaded;
        let fresh = loaded.alloc_blocks(2).expect("alloc");
        let file_extents = &loaded.objects[&2].extents;
        for extent in &fresh {
            for owned in file_extents {
                let fresh_end = extent.lb + u64::from(extent.blocks);
                let owned_end = owned.lb + u64::from(owned.blocks);
                assert!(fresh_end <= owned.lb || extent.lb >= owned_end, "overlap");
            }
        }
    }

    #[test]
    fn test_reject_corrupt_checkpoint() {
        let state = State::new_empty(256, 10);
        let mut blob = encode(&state, 1);
        blob.push(0xFF); // trailing garbage
        assert!(decode(&blob, 256, 10).is_err());
        let blob = encode(&state, 1);
        assert!(decode(&blob[..blob.len() - 1], 256, 10).is_err());
        // Extent outside the container: apply itself rejects it, and a blob
        // carrying one (forged on-disk bytes) is rejected at decode.
        let mut state = State::new_empty(256, 10);
        state
            .apply(&Op::MkNode { parent: ROOT_OBJECT, id: 2, kind: KIND_FILE, name: "f".into() })
            .expect("mknode");
        assert_eq!(
            state.apply(&Op::Write {
                id: 2,
                size: 10,
                extents: alloc::vec![Extent { lb: 500, blocks: 1 }],
            }),
            Err(NxfsError::Integrity)
        );
        state.objects.get_mut(&2).expect("object").extents.push(Extent { lb: 500, blocks: 1 });
        let blob = encode(&state, 1);
        assert_eq!(decode(&blob, 256, 10).unwrap_err(), NxfsError::Integrity);
    }
}
